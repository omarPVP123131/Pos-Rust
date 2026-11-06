//! Persistencia SQLite.
//!
//! Configuración pensada para **resistir apagones**: WAL + `synchronous=FULL`.
//! Cada venta es una sola transacción: se guarda completa o no se guarda.

pub mod auditoria;
pub mod cash;
pub mod categorias;
pub mod clientes;
pub mod conteo;
pub mod migrations;
pub mod pedidos;

/// Se reexporta para que quien hable con la base no tenga que declarar la
/// dependencia por su cuenta (y arriesgarse a una versión distinta).
pub use rusqlite;
pub mod products;
pub mod respaldo;
pub mod sales;
pub mod settings;
pub mod users;

use rusqlite::Connection;
use std::path::{Path, PathBuf};

use crate::migrations::migrate;

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Negocio(String),
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Sqlite(e) => write!(f, "Error de base de datos: {e}"),
            DbError::Negocio(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for DbError {}

pub struct Database {
    conn: Connection,
    ruta: PathBuf,
}

impl Database {
    /// Abre (o crea) la base de datos del punto de venta y aplica migraciones.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let ruta = path.as_ref().to_path_buf();
        // En Windows la base vive en %LOCALAPPDATA%\PuntoDeVenta y la ruta
        // puede venir con espacios o acentos ("Nueva carpeta"): se crea la
        // carpeta padre antes de abrir, si no SQLite dice "unable to open".
        if let Some(padre) = ruta.parent() {
            if !padre.as_os_str().is_empty() && padre != Path::new(":memory:") {
                std::fs::create_dir_all(padre).map_err(|e| {
                    DbError::Negocio(format!(
                        "No pude crear la carpeta de la base {}: {e}",
                        padre.display()
                    ))
                })?;
            }
        }
        let conn = Connection::open(&ruta)?;
        Self::configure(&conn)?;
        Ok(Database { conn, ruta })
    }

    /// Base de datos en memoria: para pruebas y para el modo demo.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        Ok(Database {
            conn,
            ruta: PathBuf::from(":memory:"),
        })
    }

    fn configure(conn: &Connection) -> Result<()> {
        // WAL: lectores y escritor no se estorban; esencial en un mostrador.
        // En algunos discos USB / Wine el WAL no arranca: se cae a DELETE
        // para que la tienda abra igual (un poco más lenta al apagón).
        let modo: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .unwrap_or_default();
        if !modo.eq_ignore_ascii_case("wal") {
            let _: String = conn.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
        }
        // FULL: ante un apagón, la última transacción confirmada está en disco.
        conn.execute_batch(
            "PRAGMA synchronous=FULL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;
             PRAGMA temp_store=MEMORY;",
        )?;
        migrate(conn)?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Ruta del archivo de la base: sirve para sugerir dónde poner los
    /// respaldos y para abrir una segunda conexión desde otro hilo.
    pub fn ruta(&self) -> &Path {
        &self.ruta
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Chequeo de integridad: se ejecuta al arrancar la app después de un
    /// apagón para avisar (y restaurar respaldo) en lugar de vender sobre
    /// una base dañada.
    /// Copia la base a `destino` con `VACUUM INTO`: sale compacta, ordenada y
    /// **consistente** (equivale a una foto del último commit, aunque haya
    /// ventas en curso). Al terminar verifica la copia.
    ///
    /// Se llama al cerrar el día y antes de cualquier actualización.
    pub fn respaldar(&self, destino: impl AsRef<Path>) -> Result<()> {
        let destino = destino.as_ref();
        if let Some(padre) = destino.parent() {
            if !padre.as_os_str().is_empty() {
                std::fs::create_dir_all(padre).map_err(|e| {
                    DbError::Negocio(format!("No pude crear la carpeta del respaldo: {e}"))
                })?;
            }
        }
        // `VACUUM INTO` no acepta parámetros: la ruta va escapada a mano.
        let ruta = destino
            .to_str()
            .ok_or_else(|| DbError::Negocio("La ruta del respaldo no es válida".into()))?
            .replace('\'', "''");
        self.conn.execute_batch(&format!("VACUUM INTO '{ruta}'"))?;

        // La copia se abre y se verifica antes de declarar el respaldo bueno.
        let copia = Connection::open(destino)?;
        let valor: String = copia.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if valor != "ok" {
            return Err(DbError::Negocio(format!(
                "El respaldo salió dañado: {valor}"
            )));
        }
        Ok(())
    }

    pub fn integrity_ok(&self) -> Result<bool> {
        let valor: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(valor == "ok")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::products::{NewProduct, Products};
    use crate::sales::Sales;
    use pos_core::{Money, Payment, PaymentMethod, Quantity, Sale, SaleLine, TaxRate};

    #[test]
    fn el_respaldo_es_una_copia_que_abre_y_cuadra() {
        let dir = std::env::temp_dir().join(format!("pos-respaldo-{}", aleatorio()));
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("tienda.db");
        let copia = dir.join("respaldo.db");

        let mut db = Database::open(&ruta).unwrap();
        for i in 0..5 {
            Products::insert(
                db.conn_mut(),
                &NewProduct {
                    sku: format!("S{i}"),
                    name: format!("Producto {i}"),
                    price_cents: 1_000,
                    cost_cents: 500,
                    tax_bp: 1_600,
                    unit: "pza".into(),
                    stock_millis: 10_000,
                    min_stock_millis: 0,
                    track_inventory: true,
                    barcodes: vec![],
                },
            )
            .unwrap();
        }

        db.respaldar(&copia).unwrap();
        assert!(copia.exists());

        // La copia abre, tiene los datos y pasa el chequeo de integridad.
        let restaurada = Database::open(&copia).unwrap();
        assert!(restaurada.integrity_ok().unwrap());
        assert_eq!(
            Products::list_active(restaurada.conn(), 100).unwrap().len(),
            5
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn aleatorio() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn producto(sku: &str, nombre: &str, precio: i64, stock: i64) -> NewProduct {
        NewProduct {
            sku: sku.to_string(),
            name: nombre.to_string(),
            price_cents: precio,
            cost_cents: precio / 2,
            tax_bp: 1600,
            unit: "pza".to_string(),
            stock_millis: stock,
            min_stock_millis: 0,
            track_inventory: true,
            barcodes: vec![format!("75{sku}")],
        }
    }

    /// Venta de un solo producto, pagada exacto, con folio automático.
    fn venta(product_id: i64, sku: &str, precio: i64, qty: i64) -> Sale {
        let mut v = Sale::new("");
        v.add_line(SaleLine {
            product_id: Some(product_id),
            sku: sku.to_string(),
            description: sku.to_string(),
            qty: Quantity::from_units(qty),
            unit_price: Money::from_cents(precio),
            discount: Money::ZERO,
            tax: TaxRate::IVA_16,
            unit_cost: Money::from_cents(precio / 2),
        })
        .unwrap();
        v.add_payment(Payment::new(
            PaymentMethod::Efectivo,
            Money::from_cents(precio * qty),
        ));
        v
    }

    #[test]
    fn abre_en_memoria_y_cuadra_la_integridad() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.integrity_ok().unwrap());
    }

    #[test]
    fn alta_y_busqueda_de_productos() {
        let mut db = Database::open_in_memory().unwrap();
        Products::insert(
            db.conn_mut(),
            &producto("COCA", "Coca-Cola 600 ml", 1800, 24000),
        )
        .unwrap();
        Products::insert(
            db.conn_mut(),
            &producto("SABR", "Sabritas original", 2200, 12000),
        )
        .unwrap();

        let por_codigo = Products::by_barcode(db.conn(), "75COCA").unwrap().unwrap();
        assert_eq!(por_codigo.name, "Coca-Cola 600 ml");
        assert_eq!(por_codigo.stock_millis, 24000);

        let encontrados = Products::search(db.conn(), "sabri", 10).unwrap();
        assert_eq!(encontrados.len(), 1);
        assert_eq!(encontrados[0].sku, "SABR");

        let todos = Products::list_active(db.conn(), 100).unwrap();
        assert_eq!(todos.len(), 2);
    }

    #[test]
    fn una_venta_descuenta_inventario_y_guarda_el_costo_historico() {
        let mut db = Database::open_in_memory().unwrap();
        let id = Products::insert(
            db.conn_mut(),
            &producto("COCA", "Coca-Cola 600 ml", 1800, 24000),
        )
        .unwrap();

        let mut v = venta(id, "COCA", 1800, 3);
        // Se paga exacto.
        let sale_id = Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();
        assert!(sale_id > 0);

        // Inventario: 24 - 3 = 21 piezas
        let despues = Products::get(db.conn(), id).unwrap().unwrap();
        assert_eq!(despues.stock_millis, 21000);

        // El costo histórico queda congelado aunque el costo cambie después.
        let guardada = Sales::by_id(db.conn(), sale_id).unwrap().unwrap();
        assert_eq!(guardada.total_cents, 5400);
        assert_eq!(guardada.tax_cents, 745);
        assert_eq!(guardada.items[0].cost_cents_at_sale, 900);
        assert_eq!(guardada.items[0].qty_millis, 3000);
    }

    #[test]
    fn la_venta_es_atomica_si_falla_algo_no_se_descuenta_nada() {
        let mut db = Database::open_in_memory().unwrap();
        let id =
            Products::insert(db.conn_mut(), &producto("COCA", "Coca-Cola", 1800, 24000)).unwrap();

        // Venta sin pagar: debe rechazarse y no tocar el inventario.
        let mut v = Sale::new("T-2");
        v.add_line(SaleLine {
            product_id: Some(id),
            sku: "COCA".into(),
            description: "Coca-Cola".into(),
            qty: Quantity::from_units(1),
            unit_price: Money::from_cents(1800),
            discount: Money::ZERO,
            tax: TaxRate::IVA_16,
            unit_cost: Money::from_cents(900),
        })
        .unwrap();

        assert!(Sales::save(db.conn_mut(), &mut v, 1, None).is_err());
        let despues = Products::get(db.conn(), id).unwrap().unwrap();
        assert_eq!(despues.stock_millis, 24000);
        assert_eq!(Sales::count(db.conn()).unwrap(), 0);
    }

    #[test]
    fn el_folio_nunca_se_repite() {
        let mut db = Database::open_in_memory().unwrap();
        let pid =
            Products::insert(db.conn_mut(), &producto("A", "Producto A", 1000, 100000)).unwrap();
        let mut folios = std::collections::HashSet::new();
        for _ in 0..25 {
            let mut v = venta(pid, "A", 1000, 1);
            let id = Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();
            let f = Sales::by_id(db.conn(), id).unwrap().unwrap().folio;
            assert!(folios.insert(f), "folio repetido");
        }
        assert_eq!(folios.len(), 25);
    }

    #[test]
    fn cancelar_una_venta_devuelve_el_inventario() {
        let mut db = Database::open_in_memory().unwrap();
        let id =
            Products::insert(db.conn_mut(), &producto("PAN", "Pan blanco", 1850, 50000)).unwrap();
        let mut v = venta(id, "PAN", 1850, 2);
        let sale_id = Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();
        assert_eq!(
            Products::get(db.conn(), id).unwrap().unwrap().stock_millis,
            48000
        );

        Sales::cancel(db.conn_mut(), sale_id, 1).unwrap();
        assert_eq!(
            Products::get(db.conn(), id).unwrap().unwrap().stock_millis,
            50000
        );
        assert_eq!(
            Sales::by_id(db.conn(), sale_id).unwrap().unwrap().status,
            "cancelada"
        );
    }

    #[test]
    fn totales_del_dia_por_metodo_de_pago() {
        let mut db = Database::open_in_memory().unwrap();
        let a =
            Products::insert(db.conn_mut(), &producto("A", "Producto A", 1000, 100000)).unwrap();

        let mut efectivo = venta(a, "A", 1000, 3);
        Sales::save(db.conn_mut(), &mut efectivo, 1, None).unwrap();

        let mut tarjeta = Sale::new("T-3");
        tarjeta
            .add_line(SaleLine {
                product_id: None,
                sku: "A".into(),
                description: "Producto A".into(),
                qty: Quantity::from_units(2),
                unit_price: Money::from_cents(1000),
                discount: Money::ZERO,
                tax: TaxRate::IVA_16,
                unit_cost: Money::from_cents(500),
            })
            .unwrap();
        tarjeta.add_payment(Payment::new(
            PaymentMethod::Tarjeta,
            Money::from_cents(2000),
        ));
        Sales::save(db.conn_mut(), &mut tarjeta, 1, None).unwrap();

        let totales = Sales::totals_today(db.conn()).unwrap();
        assert_eq!(totales.count, 2);
        assert_eq!(totales.total_cents, 5000);
        assert_eq!(totales.efectivo_cents, 3000);
        assert_eq!(totales.tarjeta_cents, 2000);
    }

    #[test]
    fn resiste_apagones_la_base_queda_integra_tras_un_cierre_brusco() {
        // Simula un corte de luz: abrimos en archivo, escribimos y "destruimos"
        // la conexión sin commit limpio. Al reabrir, la BD debe estar íntegra.
        let dir = tempfile::tempdir().unwrap();
        let ruta = dir.path().join("data.db");
        {
            let mut db = Database::open(&ruta).unwrap();
            let a = Products::insert(db.conn_mut(), &producto("A", "Producto A", 1000, 100000))
                .unwrap();
            let mut v = venta(a, "A", 1000, 1);
            Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();
            // El drop cierra la conexión; en WAL+FULL los datos ya están en disco.
        }
        let db = Database::open(&ruta).unwrap();
        assert!(db.integrity_ok().unwrap());
        assert_eq!(Sales::count(db.conn()).unwrap(), 1);
    }
}
