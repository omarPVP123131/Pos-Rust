//! Clientes y cartera (fiados).
//!
//! En el retail mexicano de barrio el fiado no es un extra: es media operación.
//! Por eso el saldo vive en la base y se mueve **en la misma transacción** que
//! la venta: si se corta la luz, nadie queda debiendo de más.

use rusqlite::{params, Connection, OptionalExtension};

use crate::{DbError, Result};
use pos_core::Money;

#[derive(Clone, Debug, PartialEq)]
pub struct Cliente {
    pub id: i64,
    pub name: String,
    pub rfc: String,
    pub phone: String,
    pub email: String,
    pub credit_limit_cents: i64,
    pub balance_cents: i64,
    pub is_active: bool,
    /// Clave de `c_RegimenFiscal`. Vacío = todavía no se puede facturarle.
    pub regimen_fiscal: String,
    /// Código postal de su domicilio fiscal: el SAT lo pide en 4.0.
    pub codigo_postal: String,
    /// Clave de `c_UsoCFDI` (S01, G03, G01...).
    pub uso_cfdi: String,
}

impl Cliente {
    /// Cuánto puede fiar todavía.
    pub fn disponible(&self) -> Money {
        let d = self.credit_limit_cents - self.balance_cents;
        Money::from_cents(d.max(0))
    }

    pub fn saldo(&self) -> Money {
        Money::from_cents(self.balance_cents)
    }

    pub fn limite(&self) -> Money {
        Money::from_cents(self.credit_limit_cents)
    }

    /// El saldo nunca debería pasar el límite; si pasa, algo está mal.
    pub fn excedido(&self) -> bool {
        self.balance_cents > self.credit_limit_cents
    }
}

#[derive(Clone, Debug)]
pub struct NuevoCliente {
    pub name: String,
    pub rfc: String,
    pub phone: String,
    pub email: String,
    pub credit_limit_cents: i64,
    pub regimen_fiscal: String,
    pub codigo_postal: String,
    pub uso_cfdi: String,
}

/// Uso CFDI cuando el cliente no dice otro: "sin efectos fiscales".
pub const USO_CFDI_OMISION: &str = "S01";

/// Un uso CFDI en blanco se toma como el de omisión, no como error: la
/// factura se puede emitir y el cliente lo corrige después si hace falta.
fn normalizar_uso(uso: &str) -> String {
    let u = uso.trim().to_uppercase();
    if u.is_empty() {
        USO_CFDI_OMISION.to_string()
    } else {
        u
    }
}

/// RFC que el SAT usa para "público en general".
pub const RFC_PUBLICO: &str = "XAXX010101000";

/// ¿A este cliente se le puede facturar con su RFC?
///
/// Hacen falta las cuatro cosas: RFC de verdad, régimen, código postal y uso
/// CFDI. Con una que falte, la venta va a la factura global del día.
pub fn datos_fiscales_completos(conn: &Connection, cliente_id: i64) -> bool {
    match Clientes::por_id(conn, cliente_id) {
        Ok(Some(c)) => {
            let rfc = c.rfc.trim().to_uppercase();
            !rfc.is_empty()
                && rfc != RFC_PUBLICO
                && !c.regimen_fiscal.trim().is_empty()
                && c.codigo_postal.trim().len() == 5
        }
        _ => false,
    }
}

pub struct Clientes;

impl Clientes {
    pub fn crear(conn: &mut Connection, c: &NuevoCliente) -> Result<i64> {
        if c.name.trim().is_empty() {
            return Err(DbError::Negocio("El cliente necesita un nombre".into()));
        }
        if c.credit_limit_cents < 0 {
            return Err(DbError::Negocio("El límite no puede ser negativo".into()));
        }
        conn.execute(
            "INSERT INTO customers (name, rfc, phone, email, credit_limit_cents,
                                    regimen_fiscal, codigo_postal, uso_cfdi)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                c.name.trim(),
                c.rfc.trim().to_uppercase(),
                c.phone.trim(),
                c.email.trim(),
                c.credit_limit_cents,
                c.regimen_fiscal.trim(),
                c.codigo_postal.trim(),
                normalizar_uso(&c.uso_cfdi),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn por_id(conn: &Connection, id: i64) -> Result<Option<Cliente>> {
        conn.query_row(
            "SELECT id, name, rfc, phone, email, credit_limit_cents, balance_cents, is_active,
                        regimen_fiscal, codigo_postal, uso_cfdi
             FROM customers WHERE id = ?1",
            params![id],
            mapear,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Búsqueda por nombre o teléfono: el mostrador busca por "Doña Mary" o
    /// por los últimos 4 dígitos del celular.
    pub fn buscar(conn: &Connection, termino: &str, limite: usize) -> Result<Vec<Cliente>> {
        let like = format!("%{}%", termino.trim());
        let mut stmt = conn.prepare(
            "SELECT id, name, rfc, phone, email, credit_limit_cents, balance_cents, is_active,
                        regimen_fiscal, codigo_postal, uso_cfdi
             FROM customers
             WHERE is_active = 1 AND (name LIKE ?1 OR phone LIKE ?1)
             ORDER BY name LIMIT ?2",
        )?;
        let filas = stmt
            .query_map(params![like, limite as i64], mapear)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    /// Clientes que deben algo, ordenados por lo que deben.
    pub fn con_saldo(conn: &Connection, limite: usize) -> Result<Vec<Cliente>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, rfc, phone, email, credit_limit_cents, balance_cents, is_active,
                        regimen_fiscal, codigo_postal, uso_cfdi
             FROM customers WHERE is_active = 1 AND balance_cents > 0
             ORDER BY balance_cents DESC LIMIT ?1",
        )?;
        let filas = stmt
            .query_map(params![limite as i64], mapear)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    pub fn actualizar_limite(conn: &Connection, id: i64, limite: Money) -> Result<()> {
        if limite.cents() < 0 {
            return Err(DbError::Negocio("El límite no puede ser negativo".into()));
        }
        conn.execute(
            "UPDATE customers SET credit_limit_cents = ?1 WHERE id = ?2",
            params![limite.cents(), id],
        )?;
        Ok(())
    }

    /// Registra un abono: baja el saldo y deja constancia.
    pub fn abonar(
        conn: &mut Connection,
        id: i64,
        monto: Money,
        metodo: &str,
        user_id: i64,
    ) -> Result<Money> {
        if monto.cents() <= 0 {
            return Err(DbError::Negocio("El abono debe ser mayor a cero".into()));
        }
        let cliente =
            Self::por_id(conn, id)?.ok_or(DbError::Negocio("No existe ese cliente".into()))?;
        if monto.cents() > cliente.balance_cents {
            return Err(DbError::Negocio(format!(
                "El abono ({monto}) es mayor que la deuda ({})",
                cliente.saldo()
            )));
        }

        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE customers SET balance_cents = balance_cents - ?1 WHERE id = ?2",
            params![monto.cents(), id],
        )?;
        tx.execute(
            "INSERT INTO customer_payments (customer_id, amount_cents, method, user_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, monto.cents(), metodo, user_id],
        )?;
        tx.execute(
            "INSERT INTO audit_log (user_id, action, entity, entity_id, payload_json)
             VALUES (?1, 'abono', 'customer', ?2, ?3)",
            params![user_id, id, format!("{}|{}", monto.cents(), metodo)],
        )?;
        tx.commit()?;

        let saldo = Self::por_id(conn, id)?
            .map(|c| c.saldo())
            .unwrap_or_default();
        Ok(saldo)
    }

    /// Suma fiada y no pagada: el dinero que está en la calle.
    pub fn total_cartera(conn: &Connection) -> Result<Money> {
        let total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(balance_cents), 0) FROM customers WHERE is_active = 1",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok(Money::from_cents(total))
    }

    pub fn desactivar(conn: &Connection, id: i64) -> Result<()> {
        let saldo = Self::por_id(conn, id)?
            .map(|c| c.balance_cents)
            .unwrap_or(0);
        if saldo != 0 {
            return Err(DbError::Negocio(
                "No se puede dar de baja con saldo pendiente".into(),
            ));
        }
        conn.execute(
            "UPDATE customers SET is_active = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }
}

fn mapear(row: &rusqlite::Row<'_>) -> rusqlite::Result<Cliente> {
    Ok(Cliente {
        id: row.get(0)?,
        name: row.get(1)?,
        rfc: row.get(2)?,
        phone: row.get(3)?,
        email: row.get(4)?,
        credit_limit_cents: row.get(5)?,
        balance_cents: row.get(6)?,
        is_active: row.get::<_, i32>(7)? != 0,
        regimen_fiscal: row.get(8).unwrap_or_default(),
        codigo_postal: row.get(9).unwrap_or_default(),
        uso_cfdi: row.get(10).unwrap_or_else(|_| USO_CFDI_OMISION.into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cash::Cash;
    use crate::products::{NewProduct, Products};
    use crate::sales::Sales;
    use crate::Database;
    use pos_core::Arqueo;
    use pos_core::{Payment, PaymentMethod, Quantity, Sale, SaleLine, TaxRate};

    fn cliente(nombre: &str, limite: i64) -> NuevoCliente {
        NuevoCliente {
            name: nombre.into(),
            rfc: String::new(),
            phone: "5512345678".into(),
            email: String::new(),
            credit_limit_cents: limite,
            regimen_fiscal: String::new(),
            codigo_postal: String::new(),
            uso_cfdi: String::new(),
        }
    }

    fn alta_producto(db: &mut Database, sku: &str, precio: i64) -> i64 {
        Products::insert(
            db.conn_mut(),
            &NewProduct {
                sku: sku.into(),
                name: format!("Producto {sku}"),
                price_cents: precio,
                cost_cents: precio / 2,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: 1_000_000,
                min_stock_millis: 0,
                track_inventory: true,
                barcodes: vec![],
            },
        )
        .unwrap()
    }

    fn venta_a_credito(
        db: &mut Database,
        product_id: i64,
        sku: &str,
        precio: i64,
        qty: i64,
        cliente_id: i64,
    ) -> i64 {
        let mut v = Sale::new("");
        v.customer_id = Some(cliente_id);
        v.add_line(SaleLine {
            product_id: Some(product_id),
            sku: sku.into(),
            description: sku.into(),
            qty: Quantity::from_units(qty),
            unit_price: Money::from_cents(precio),
            discount: Money::ZERO,
            tax: TaxRate::IVA_16,
            unit_cost: Money::from_cents(precio / 2),
        })
        .unwrap();
        v.add_payment(Payment::new(
            PaymentMethod::Credito,
            Money::from_cents(precio * qty),
        ));
        Sales::save(db.conn_mut(), &mut v, 1, None).unwrap()
    }

    #[test]
    fn alta_busqueda_y_limite_disponible() {
        let mut db = Database::open_in_memory().unwrap();
        let id = Clientes::crear(db.conn_mut(), &cliente("Doña Mary", 50_000)).unwrap();

        let c = Clientes::por_id(db.conn(), id).unwrap().unwrap();
        assert_eq!(c.disponible(), Money::from_cents(50_000));
        assert!(!c.excedido());

        let encontrados = Clientes::buscar(db.conn(), "Mary", 10).unwrap();
        assert_eq!(encontrados.len(), 1);
        assert_eq!(Clientes::buscar(db.conn(), "5678", 10).unwrap().len(), 1);
    }

    #[test]
    fn vender_a_credito_aumenta_el_saldo_en_la_misma_transaccion() {
        let mut db = Database::open_in_memory().unwrap();
        let cid = Clientes::crear(db.conn_mut(), &cliente("Don Pepe", 100_000)).unwrap();
        let pid = alta_producto(&mut db, "A", 1000);

        venta_a_credito(&mut db, pid, "A", 1000, 30, cid); // fió $300

        let c = Clientes::por_id(db.conn(), cid).unwrap().unwrap();
        assert_eq!(c.saldo(), Money::from_cents(30_000));
        assert_eq!(c.disponible(), Money::from_cents(70_000));
        assert_eq!(
            Clientes::total_cartera(db.conn()).unwrap(),
            Money::from_cents(30_000)
        );
    }

    #[test]
    fn abonar_baja_el_saldo_y_queda_registrado() {
        let mut db = Database::open_in_memory().unwrap();
        let cid = Clientes::crear(db.conn_mut(), &cliente("Doña Mary", 100_000)).unwrap();
        let pid = alta_producto(&mut db, "A", 1000);
        venta_a_credito(&mut db, pid, "A", 1000, 50, cid); // debe $500

        let saldo =
            Clientes::abonar(db.conn_mut(), cid, Money::from_cents(20_000), "efectivo", 1).unwrap();
        assert_eq!(saldo, Money::from_cents(30_000));

        // No se puede abonar más de lo que debe.
        assert!(Clientes::abonar(
            db.conn_mut(),
            cid,
            Money::from_cents(999_999),
            "efectivo",
            1
        )
        .is_err());
        // Ni abonos en cero o negativos.
        assert!(Clientes::abonar(db.conn_mut(), cid, Money::ZERO, "efectivo", 1).is_err());
    }

    #[test]
    fn los_clientes_con_saldo_aparecen_primero() {
        let mut db = Database::open_in_memory().unwrap();
        let mary = Clientes::crear(db.conn_mut(), &cliente("Mary", 100_000)).unwrap();
        let pepe = Clientes::crear(db.conn_mut(), &cliente("Pepe", 100_000)).unwrap();
        let sin_deuda = Clientes::crear(db.conn_mut(), &cliente("Ana", 100_000)).unwrap();
        let pid = alta_producto(&mut db, "A", 1000);

        venta_a_credito(&mut db, pid, "A", 1000, 10, mary); // $100
        venta_a_credito(&mut db, pid, "A", 1000, 25, pepe); // $250

        let lista = Clientes::con_saldo(db.conn(), 10).unwrap();
        assert_eq!(lista.len(), 2);
        assert_eq!(lista[0].name, "Pepe"); // debe más
        assert!(!lista.iter().any(|c| c.id == sin_deuda));
    }

    #[test]
    fn no_se_da_de_baja_un_cliente_con_deuda() {
        let mut db = Database::open_in_memory().unwrap();
        let cid = Clientes::crear(db.conn_mut(), &cliente("Pepe", 100_000)).unwrap();
        let pid = alta_producto(&mut db, "A", 1000);
        venta_a_credito(&mut db, pid, "A", 1000, 5, cid);

        assert!(Clientes::desactivar(db.conn(), cid).is_err());
        Clientes::abonar(db.conn_mut(), cid, Money::from_cents(5_000), "efectivo", 1).unwrap();
        assert!(Clientes::desactivar(db.conn(), cid).is_ok());
    }

    #[test]
    fn el_fiado_no_se_cuenta_como_efectivo_en_la_caja() {
        // Una venta a crédito no mete dinero al cajón: si lo hiciera, el corte
        // de caja daría siempre de más.
        let mut db = Database::open_in_memory().unwrap();
        let sesion = Cash::abrir(db.conn_mut(), 1, Money::ZERO).unwrap();
        let cid = Clientes::crear(db.conn_mut(), &cliente("Pepe", 100_000)).unwrap();
        let pid = alta_producto(&mut db, "A", 1000);

        let mut v = Sale::new("");
        v.customer_id = Some(cid);
        v.add_line(SaleLine {
            product_id: Some(pid),
            sku: "A".into(),
            description: "A".into(),
            qty: Quantity::from_units(5),
            unit_price: Money::from_cents(1000),
            discount: Money::ZERO,
            tax: TaxRate::IVA_16,
            unit_cost: Money::from_cents(500),
        })
        .unwrap();
        v.add_payment(Payment::new(
            PaymentMethod::Credito,
            Money::from_cents(5000),
        ));
        Sales::save(db.conn_mut(), &mut v, 1, Some(sesion)).unwrap();

        let corte = Cash::corte_actual(db.conn(), sesion).unwrap();
        assert_eq!(corte.ventas_efectivo, Money::ZERO);
        assert_eq!(corte.esperado(), Money::ZERO);

        // Abona en efectivo $30: eso sí entra al cajón (se registra aparte).
        let mut arqueo = Arqueo::new();
        arqueo.set_piezas(2_000, 1); // un billete de $20
        arqueo.set_piezas(1_000, 1); // uno de $10
        let cierre = Cash::cerrar(db.conn_mut(), sesion, &arqueo, 1).unwrap();
        assert_eq!(cierre.contado, Money::from_cents(3_000));
        assert_eq!(cierre.sobrante(), Money::from_cents(3_000));
    }

    #[test]
    fn requiere_nombre_y_limite_valido() {
        let mut db = Database::open_in_memory().unwrap();
        let mut c = cliente("", 0);
        assert!(Clientes::crear(db.conn_mut(), &c).is_err());
        c = cliente("Juan", -100);
        assert!(Clientes::crear(db.conn_mut(), &c).is_err());
    }
}
