//! Catálogo e inventario.
//!
//! El stock de `products` es un caché; la verdad vive en `inventory_movements`.
//! Ambos se actualizan en la misma transacción, así que si algo falla, nada
//! se mueve.

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};

use crate::{DbError, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    pub id: i64,
    pub sku: String,
    pub name: String,
    pub unit: String,
    pub price_cents: i64,
    pub cost_cents: i64,
    pub tax_bp: i32,
    pub track_inventory: bool,
    pub stock_millis: i64,
    pub min_stock_millis: i64,
    pub is_active: bool,
    /// Clave de `c_ClaveProdServ`. '01010101' = no existe en el catálogo.
    pub clave_sat: String,
    /// Clave de `c_ClaveUnidad`. 'H87' = pieza.
    pub clave_unidad: String,
    /// Cómo se nombra la unidad en el comprobante: "Pieza", "Kilo"...
    pub unidad_sat: String,
}

impl Product {
    /// Precio de mostrador (con impuesto incluido).
    pub fn price(&self) -> pos_core::Money {
        pos_core::Money::from_cents(self.price_cents)
    }

    pub fn below_min_stock(&self) -> bool {
        self.track_inventory && self.stock_millis <= self.min_stock_millis
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewProduct {
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
    pub cost_cents: i64,
    pub tax_bp: i32,
    pub unit: String,
    pub stock_millis: i64,
    pub min_stock_millis: i64,
    pub track_inventory: bool,
    pub barcodes: Vec<String>,
}

/// Campos editables de un producto ya existente (el SKU también puede
/// corregirse; el stock NO: para eso están los movimientos de inventario).
#[derive(Clone, Debug, Default)]
pub struct ProductUpdate {
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
    pub cost_cents: i64,
    pub tax_bp: i32,
    pub unit: String,
    pub min_stock_millis: i64,
    pub track_inventory: bool,
}

const SELECT_PRODUCT: &str = "
    SELECT p.id, p.sku, p.name, p.unit, p.price_cents, p.cost_cents,
           t.percent_bp, p.track_inventory, p.stock_millis,
           p.min_stock_millis, p.is_active,
           p.clave_sat, p.clave_unidad, p.unidad_sat
    FROM products p
    JOIN tax_rates t ON t.id = p.tax_rate_id";

fn map_row(row: &Row<'_>) -> rusqlite::Result<Product> {
    Ok(Product {
        id: row.get("id")?,
        sku: row.get("sku")?,
        name: row.get("name")?,
        unit: row.get("unit")?,
        price_cents: row.get("price_cents")?,
        cost_cents: row.get("cost_cents")?,
        tax_bp: row.get("percent_bp")?,
        track_inventory: row.get::<_, i32>("track_inventory")? != 0,
        stock_millis: row.get("stock_millis")?,
        min_stock_millis: row.get("min_stock_millis")?,
        is_active: row.get::<_, i32>("is_active")? != 0,
        clave_sat: row
            .get("clave_sat")
            .unwrap_or_else(|_| CLAVE_SAT_GENERICA.into()),
        clave_unidad: row
            .get("clave_unidad")
            .unwrap_or_else(|_| CLAVE_UNIDAD.into()),
        unidad_sat: row.get("unidad_sat").unwrap_or_else(|_| "Pieza".into()),
    })
}

/// Clave genérica de `c_ClaveProdServ`: "no existe en el catálogo".
pub const CLAVE_SAT_GENERICA: &str = "01010101";
/// Clave de `c_ClaveUnidad` para pieza.
pub const CLAVE_UNIDAD: &str = "H87";

/// Cuánto se ha devuelto ya (en notas de crédito) de una venta, por producto.
pub fn devuelto_por_notas(conn: &Connection, folio: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT n.sku, SUM(n.cantidad_millis)
         FROM notas_lineas n
         JOIN facturas f ON f.id = n.factura_id
         WHERE f.ventas = ?1 AND f.tipo = 'egreso' AND f.estado <> 'error'
         GROUP BY n.sku",
    )?;
    let mut salida = Vec::new();
    for fila in stmt.query_map(params![folio], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        salida.push(fila?);
    }
    Ok(salida)
}

pub struct Products;

impl Products {
    /// Guarda las claves del SAT de un producto.
    ///
    /// Se guardan tal cual las escribió el usuario, sin validar contra el catálogo
    /// oficial: si la clave está mal, el PAC la rechaza al timbrar y ahí se ve.
    /// Inventar una validación propia daría falsos positivos.
    pub fn set_claves_sat(
        conn: &Connection,
        producto_id: i64,
        clave_sat: &str,
        clave_unidad: &str,
        unidad_sat: &str,
    ) -> Result<()> {
        conn.execute(
            "UPDATE products SET clave_sat = ?1, clave_unidad = ?2, unidad_sat = ?3 WHERE id = ?4",
            params![
                clave_sat.trim(),
                clave_unidad.trim(),
                unidad_sat.trim(),
                producto_id
            ],
        )?;
        Ok(())
    }
    /// Da de alta un producto con sus códigos de barras y, si hay existencia
    /// inicial, el movimiento de inventario correspondiente.
    pub fn insert(conn: &mut Connection, p: &NewProduct) -> Result<i64> {
        if p.sku.trim().is_empty() || p.name.trim().is_empty() {
            return Err(DbError::Negocio("El producto necesita SKU y nombre".into()));
        }
        let tax_rate_id: i64 = conn.query_row(
            "SELECT id FROM tax_rates WHERE percent_bp = ?1 LIMIT 1",
            params![p.tax_bp],
            |r| r.get(0),
        )?;

        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO products
             (sku, name, unit, price_cents, cost_cents, tax_rate_id,
              track_inventory, stock_millis, min_stock_millis)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                p.sku,
                p.name,
                p.unit,
                p.price_cents,
                p.cost_cents,
                tax_rate_id,
                p.track_inventory as i32,
                if p.track_inventory { p.stock_millis } else { 0 },
                p.min_stock_millis,
            ],
        )?;
        let id = tx.last_insert_rowid();

        for codigo in &p.barcodes {
            if codigo.trim().is_empty() {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO barcodes (product_id, code) VALUES (?1, ?2)",
                params![id, codigo.trim()],
            )?;
        }

        if p.track_inventory && p.stock_millis != 0 {
            tx.execute(
                "INSERT INTO inventory_movements
                 (product_id, qty_delta_millis, reason, unit_cost_cents, ref_type)
                 VALUES (?1, ?2, 'inicial', ?3, 'alta')",
                params![id, p.stock_millis, p.cost_cents],
            )?;
        }

        tx.execute(
            "INSERT INTO audit_log (action, entity, entity_id) VALUES ('alta', 'product', ?1)",
            params![id],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn get(conn: &Connection, id: i64) -> Result<Option<Product>> {
        conn.query_row(
            &format!("{SELECT_PRODUCT} WHERE p.id = ?1"),
            params![id],
            map_row,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Búsqueda por SKU exacto: el camino de la app móvil y del carrito.
    pub fn by_sku(conn: &Connection, sku: &str) -> Result<Option<Product>> {
        let mut stmt = conn.prepare_cached(&format!(
            "{SELECT_PRODUCT} WHERE p.sku = ?1 AND p.is_active = 1"
        ))?;
        stmt.query_row(params![sku.trim()], map_row)
            .optional()
            .map_err(Into::into)
    }

    /// Búsqueda por código de barras exacto: el camino del escáner.
    pub fn by_barcode(conn: &Connection, code: &str) -> Result<Option<Product>> {
        let mut stmt = conn.prepare_cached(&format!(
            "{SELECT_PRODUCT}
             JOIN barcodes b ON b.product_id = p.id
             WHERE b.code = ?1 AND p.is_active = 1"
        ))?;
        stmt.query_row(params![code.trim()], map_row)
            .optional()
            .map_err(Into::into)
    }

    /// Búsqueda por nombre o SKU para la captura manual.
    pub fn search(conn: &Connection, term: &str, limit: usize) -> Result<Vec<Product>> {
        let term = term.trim();
        if term.is_empty() {
            return Self::list_active(conn, limit);
        }
        let like = format!("%{term}%");
        let mut stmt = conn.prepare_cached(&format!(
            "{SELECT_PRODUCT}
             WHERE p.is_active = 1 AND (
                 p.name LIKE ?1 OR p.sku LIKE ?1
                 OR EXISTS (SELECT 1 FROM barcodes b WHERE b.product_id = p.id AND b.code LIKE ?1)
             )
             ORDER BY p.name LIMIT ?2"
        ))?;
        let filas = stmt
            .query_map(params![like, limit as i64], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    pub fn list_active(conn: &Connection, limit: usize) -> Result<Vec<Product>> {
        let mut stmt = conn.prepare_cached(&format!(
            "{SELECT_PRODUCT} WHERE p.is_active = 1 ORDER BY p.name LIMIT ?1"
        ))?;
        let filas = stmt
            .query_map(params![limit as i64], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    /// Productos en o por debajo del mínimo: la alerta del mostrador.
    pub fn low_stock(conn: &Connection, limit: usize) -> Result<Vec<Product>> {
        let mut stmt = conn.prepare_cached(&format!(
            "{SELECT_PRODUCT}
             WHERE p.is_active = 1 AND p.track_inventory = 1
               AND p.stock_millis <= p.min_stock_millis
             ORDER BY p.stock_millis ASC LIMIT ?1"
        ))?;
        let filas = stmt
            .query_map(params![limit as i64], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    pub fn deactivate(conn: &Connection, id: i64) -> Result<()> {
        conn.execute(
            "UPDATE products SET is_active = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Códigos de barras de un producto.
    pub fn barcodes_de(conn: &Connection, id: i64) -> Result<Vec<String>> {
        let mut stmt =
            conn.prepare("SELECT code FROM barcodes WHERE product_id = ?1 ORDER BY code")?;
        let filas = stmt
            .query_map(params![id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    /// Siguiente EAN-13 interno (prefijo 20) que no esté usado.
    ///
    /// Sirve para granel, ropa y todo lo que llega sin código de fábrica. El
    /// lector lo cobra igual: entra por `barcodes` como cualquiera.
    pub fn siguiente_ean_interno(conn: &Connection) -> Result<String> {
        let mut stmt = conn.prepare(
            "SELECT code FROM barcodes
             WHERE length(code) = 13 AND code GLOB '20[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]'",
        )?;
        let usados: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut max_n: u64 = 0;
        for c in &usados {
            if let Ok(n) = c[2..12].parse::<u64>() {
                max_n = max_n.max(n);
            }
        }
        for n in (max_n + 1)..=9_999_999_999 {
            let Some(codigo) = pos_core::ean13_interno(n) else {
                continue;
            };
            let en_barras: i64 = conn.query_row(
                "SELECT COUNT(*) FROM barcodes WHERE code = ?1",
                rusqlite::params![codigo],
                |r| r.get(0),
            )?;
            let en_sku: i64 = conn.query_row(
                "SELECT COUNT(*) FROM products WHERE sku = ?1",
                rusqlite::params![codigo],
                |r| r.get(0),
            )?;
            if en_barras == 0 && en_sku == 0 {
                return Ok(codigo);
            }
        }
        Err(crate::DbError::Negocio(
            "Se acabaron los códigos internos (20 + 10 dígitos).".into(),
        ))
    }

    /// Reemplaza los códigos de barras del producto por esta lista.
    pub fn set_barcodes(conn: &Connection, id: i64, codigos: &[String]) -> Result<()> {
        conn.execute("DELETE FROM barcodes WHERE product_id = ?1", params![id])?;
        let mut stmt =
            conn.prepare("INSERT OR IGNORE INTO barcodes (product_id, code) VALUES (?1, ?2)")?;
        for c in codigos {
            if c.trim().is_empty() {
                continue;
            }
            stmt.execute(params![id, c.trim()])?;
        }
        Ok(())
    }

    /// Edita un producto existente (precio, costo, nombre, mínimo...).
    pub fn update(conn: &mut Connection, id: i64, p: &ProductUpdate) -> Result<()> {
        if p.sku.trim().is_empty() || p.name.trim().is_empty() {
            return Err(DbError::Negocio("El producto necesita SKU y nombre".into()));
        }
        let tax_rate_id: i64 = conn.query_row(
            "SELECT id FROM tax_rates WHERE percent_bp = ?1 LIMIT 1",
            params![p.tax_bp],
            |r| r.get(0),
        )?;

        let tx = conn.transaction()?;
        let cambiaron = tx.execute(
            "UPDATE products SET sku = ?1, name = ?2, unit = ?3, price_cents = ?4,
                    cost_cents = ?5, tax_rate_id = ?6, track_inventory = ?7,
                    min_stock_millis = ?8, updated_at = datetime('now')
             WHERE id = ?9",
            params![
                p.sku.trim(),
                p.name.trim(),
                p.unit,
                p.price_cents,
                p.cost_cents,
                tax_rate_id,
                p.track_inventory as i32,
                p.min_stock_millis,
                id,
            ],
        )?;
        if cambiaron == 0 {
            return Err(DbError::Negocio("Ese producto ya no existe".into()));
        }
        tx.execute(
            "INSERT INTO audit_log (action, entity, entity_id) VALUES ('edicion', 'product', ?1)",
            params![id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Importación masiva: **todo dentro de una sola transacción**.
    ///
    /// Si un renglón falla, no se escribe nada y el error dice el número de
    /// renglón (contando el encabezado) para que el usuario lo corrija en su
    /// hoja de cálculo.
    ///
    /// Devuelve `(creados, actualizados)`.
    pub fn importar(
        conn: &mut Connection,
        filas: &[NewProduct],
        actualizar: bool,
    ) -> Result<(usize, usize)> {
        let mut impuestos: std::collections::HashMap<i32, i64> = std::collections::HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT id, percent_bp FROM tax_rates")?;
            let todas = stmt
                .query_map([], |r| Ok((r.get::<_, i32>(1)?, r.get::<_, i64>(0)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (bp, id) in todas {
                impuestos.insert(bp, id);
            }
        }

        let tx = conn.transaction()?;
        let (creados, actualizados) = {
            let mut buscar = tx.prepare("SELECT id FROM products WHERE sku = ?1")?;
            let mut insertar = tx.prepare(
                "INSERT INTO products
                 (sku, name, unit, price_cents, cost_cents, tax_rate_id,
                  track_inventory, stock_millis, min_stock_millis)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut actualizar_stmt = tx.prepare(
                "UPDATE products SET name = ?2, unit = ?3, price_cents = ?4,
                        cost_cents = ?5, tax_rate_id = ?6, track_inventory = ?7,
                        min_stock_millis = ?8, is_active = 1,
                        updated_at = datetime('now')
                 WHERE id = ?1",
            )?;
            let mut codigo_stmt =
                tx.prepare("INSERT OR IGNORE INTO barcodes (product_id, code) VALUES (?1, ?2)")?;
            let mut movimiento_stmt = tx.prepare(
                "INSERT INTO inventory_movements
                 (product_id, qty_delta_millis, reason, unit_cost_cents, ref_type)
                 VALUES (?1, ?2, ?, ?3, 'importacion')",
            )?;

            let mut creados = 0usize;
            let mut actualizados = 0usize;

            for (i, p) in filas.iter().enumerate() {
                let renglon = i + 2; // +1 por el encabezado, +1 porque se cuenta desde 1
                if p.sku.trim().is_empty() || p.name.trim().is_empty() {
                    return Err(DbError::Negocio(format!(
                        "Renglón {renglon}: falta el SKU o el nombre"
                    )));
                }
                let tax_rate_id = *impuestos.get(&p.tax_bp).ok_or_else(|| {
                    DbError::Negocio(format!(
                        "Renglón {renglon}: no existe una tasa de impuesto de {}%",
                        p.tax_bp as f64 / 100.0
                    ))
                })?;

                let existente: Option<i64> = buscar
                    .query_row(params![p.sku.trim()], |r| r.get(0))
                    .optional()?;

                let id = match existente {
                    Some(id) => {
                        if !actualizar {
                            return Err(DbError::Negocio(format!(
                                "Renglón {renglon}: el SKU '{}' ya existe (marca la opción de actualizar para sobreescribirlo)",
                                p.sku
                            )));
                        }
                        actualizar_stmt.execute(params![
                            id,
                            p.name.trim(),
                            p.unit,
                            p.price_cents,
                            p.cost_cents,
                            tax_rate_id,
                            p.track_inventory as i32,
                            p.min_stock_millis,
                        ])?;
                        actualizados += 1;
                        id
                    }
                    None => {
                        insertar.execute(params![
                            p.sku.trim(),
                            p.name.trim(),
                            p.unit,
                            p.price_cents,
                            p.cost_cents,
                            tax_rate_id,
                            p.track_inventory as i32,
                            if p.track_inventory { p.stock_millis } else { 0 },
                            p.min_stock_millis,
                        ])?;
                        creados += 1;
                        tx.last_insert_rowid()
                    }
                };

                for codigo in &p.barcodes {
                    if codigo.trim().is_empty() {
                        continue;
                    }
                    codigo_stmt.execute(params![id, codigo.trim()])?;
                }

                if p.track_inventory && p.stock_millis != 0 {
                    movimiento_stmt.execute(params![id, p.stock_millis, p.cost_cents])?;
                }
            }

            tx.execute(
                "INSERT INTO audit_log (action, entity, entity_id)
                 VALUES ('importacion_csv', 'product', ?1)",
                params![filas.len() as i64],
            )?;
            (creados, actualizados)
        };
        tx.commit()?;
        Ok((creados, actualizados))
    }

    /// Entrada de mercancía, ajuste o merma. Registra el movimiento y mueve
    /// el stock en la misma transacción.
    pub fn adjust_stock(
        conn: &mut Connection,
        id: i64,
        delta_millis: i64,
        reason: &str,
        user_id: i64,
    ) -> Result<()> {
        let tx = conn.transaction()?;
        adjust_stock_tx(&tx, id, delta_millis, reason, user_id, None, None)?;
        tx.commit()?;
        Ok(())
    }
}

/// Variante que participa en una transacción mayor (por ejemplo, una venta).
///
/// `costo_cents` sobreescribe el costo unitario del movimiento (una compra
/// llega con el precio del proveedor; una venta usa el costo que ya tenía).
pub fn adjust_stock_tx(
    tx: &Transaction<'_>,
    product_id: i64,
    delta_millis: i64,
    reason: &str,
    user_id: i64,
    referencia: Option<(&str, i64)>,
    costo_cents: Option<i64>,
) -> Result<()> {
    let (tracked_i, nombre, actual, unit_cost): (i32, String, i64, i64) = tx.query_row(
        "SELECT track_inventory, name, stock_millis, cost_cents FROM products WHERE id = ?1",
        params![product_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let tracked = tracked_i != 0;
    let unit_cost = costo_cents.unwrap_or(unit_cost);

    // Vender 30 cuando hay 29 no puede dejar el anaquel en -1: el cajero
    // se entera ahora, no cuando el siguiente cliente pide la última.
    if tracked && actual + delta_millis < 0 {
        let hay = pos_core::Quantity::from_millis(actual).format();
        let pide = pos_core::Quantity::from_millis(delta_millis.abs()).format();
        let msg = if reason == "venta" {
            format!("No hay suficiente '{nombre}': hay {hay}, pides {pide}")
        } else {
            format!("El inventario de '{nombre}' no puede quedar negativo (hay {hay})")
        };
        return Err(DbError::Negocio(msg));
    }

    tx.execute(
        "INSERT INTO inventory_movements
         (product_id, qty_delta_millis, reason, unit_cost_cents, ref_type, ref_id, user_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            product_id,
            delta_millis,
            reason,
            unit_cost,
            referencia.map(|(t, _)| t),
            referencia.map(|(_, i)| i),
            user_id,
        ],
    )?;

    if tracked {
        tx.execute(
            "UPDATE products SET stock_millis = stock_millis + ?1, updated_at = datetime('now')
             WHERE id = ?2",
            params![delta_millis, product_id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn nuevo(sku: &str, precio: i64, stock: i64) -> NewProduct {
        NewProduct {
            sku: sku.into(),
            name: format!("Producto {sku}"),
            price_cents: precio,
            cost_cents: precio / 2,
            tax_bp: 1600,
            unit: "pza".into(),
            stock_millis: stock,
            min_stock_millis: 0,
            track_inventory: true,
            barcodes: vec![],
        }
    }

    #[test]
    fn el_inventario_cuadra_con_el_libro_de_movimientos() {
        let mut db = Database::open_in_memory().unwrap();
        let id = Products::insert(db.conn_mut(), &nuevo("A", 1000, 0)).unwrap();

        for (delta, motivo) in [
            (10_000i64, "compra"),
            (-3_000, "merma"),
            (500, "ajuste"),
            (-2_500, "venta"),
        ] {
            Products::adjust_stock(db.conn_mut(), id, delta, motivo, 1).unwrap();
        }

        let producto = Products::get(db.conn(), id).unwrap().unwrap();
        let libro: i64 = db
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(qty_delta_millis),0) FROM inventory_movements
                 WHERE product_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(libro, 5_000);
        assert_eq!(producto.stock_millis, libro);
    }

    #[test]
    fn detecta_stock_debajo_del_minimo() {
        let mut db = Database::open_in_memory().unwrap();
        let mut p = nuevo("A", 1000, 5_000);
        p.min_stock_millis = 10_000;
        let id = Products::insert(db.conn_mut(), &p).unwrap();
        assert!(Products::get(db.conn(), id)
            .unwrap()
            .unwrap()
            .below_min_stock());
        assert_eq!(Products::low_stock(db.conn(), 20).unwrap().len(), 1);
    }

    #[test]
    fn no_se_puede_dar_de_alta_un_producto_sin_nombre() {
        let mut db = Database::open_in_memory().unwrap();
        let mut p = nuevo("A", 1000, 0);
        p.name = "".into();
        assert!(Products::insert(db.conn_mut(), &p).is_err());
    }

    #[test]
    fn un_producto_sin_inventario_no_mueve_existencias() {
        let mut db = Database::open_in_memory().unwrap();
        let mut p = nuevo("SERV", 5000, 0);
        p.track_inventory = false;
        let id = Products::insert(db.conn_mut(), &p).unwrap();
        Products::adjust_stock(db.conn_mut(), id, -1000, "venta", 1).unwrap();
        assert_eq!(
            Products::get(db.conn(), id).unwrap().unwrap().stock_millis,
            0
        );
    }

    #[test]
    fn vender_mas_de_lo_que_hay_no_deja_el_stock_en_negativo() {
        let mut db = Database::open_in_memory().unwrap();
        // 29 piezas: vender 30 no puede pasar.
        let id = Products::insert(db.conn_mut(), &nuevo("COCA", 1800, 29_000)).unwrap();
        let err = Products::adjust_stock(db.conn_mut(), id, -30_000, "venta", 1).unwrap_err();
        let texto = err.to_string();
        assert!(
            texto.contains("No hay suficiente") && texto.contains("29") && texto.contains("30"),
            "el error tiene que decir cuánto hay y cuánto se pide: {texto}"
        );
        assert_eq!(
            Products::get(db.conn(), id).unwrap().unwrap().stock_millis,
            29_000,
            "si se rechaza, el anaquel sigue igual"
        );
    }

    #[test]
    fn vender_justo_lo_que_hay_deja_el_stock_en_cero() {
        let mut db = Database::open_in_memory().unwrap();
        let id = Products::insert(db.conn_mut(), &nuevo("COCA", 1800, 29_000)).unwrap();
        Products::adjust_stock(db.conn_mut(), id, -29_000, "venta", 1).unwrap();
        assert_eq!(
            Products::get(db.conn(), id).unwrap().unwrap().stock_millis,
            0
        );
        assert!(Products::adjust_stock(db.conn_mut(), id, -1_000, "venta", 1).is_err());
        assert_eq!(
            Products::get(db.conn(), id).unwrap().unwrap().stock_millis,
            0
        );
    }

    #[test]
    fn un_ajuste_tampoco_deja_el_stock_en_negativo() {
        let mut db = Database::open_in_memory().unwrap();
        let id = Products::insert(db.conn_mut(), &nuevo("COCA", 1800, 5_000)).unwrap();
        let err = Products::adjust_stock(db.conn_mut(), id, -6_000, "ajuste", 1).unwrap_err();
        let texto = err.to_string();
        assert!(
            texto.contains("no puede quedar negativo") && texto.contains("5"),
            "un ajuste dice cuánto hay, no 'pides': {texto}"
        );
        assert_eq!(
            Products::get(db.conn(), id).unwrap().unwrap().stock_millis,
            5_000
        );
    }

    #[test]
    fn el_codigo_interno_es_ean13_y_no_se_repite() {
        let mut db = Database::open_in_memory().unwrap();
        let a = Products::siguiente_ean_interno(db.conn()).unwrap();
        assert!(pos_core::ean13_valido(&a));
        assert!(a.starts_with("20"));
        let mut p = nuevo("GRANEL", 2500, 0);
        p.barcodes = vec![a.clone()];
        Products::insert(db.conn_mut(), &p).unwrap();
        let b = Products::siguiente_ean_interno(db.conn()).unwrap();
        assert_ne!(a, b);
        assert!(pos_core::ean13_valido(&b));
        assert!(Products::search(db.conn(), &a, 10)
            .unwrap()
            .iter()
            .any(|x| x.sku == "GRANEL"));
    }
}
