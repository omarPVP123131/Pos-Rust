//! Sesiones de caja: apertura, movimientos y corte con arqueo.
//!
//! El efectivo esperado se calcula **derivándolo de los hechos** (ventas en
//! efectivo, retiros, gastos, entradas), no guardando un acumulado que se
//! puede desincronizar. Si cuadra mal, siempre se puede explicar por qué.

use rusqlite::{params, Connection, OptionalExtension};

use crate::{DbError, Result};
use pos_core::{Arqueo, Cierre, CorteCaja, EstadoCaja, Money, MovimientoCaja};

#[derive(Clone, Debug, PartialEq)]
pub struct CashSession {
    pub id: i64,
    pub user_id: i64,
    pub opening_float_cents: i64,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub status: EstadoCaja,
}

impl CashSession {
    pub fn fondo(&self) -> Money {
        Money::from_cents(self.opening_float_cents)
    }

    pub fn abierta(&self) -> bool {
        self.status == EstadoCaja::Abierta
    }
}

pub struct Cash;

impl Cash {
    /// Abre la caja con un fondo de cambio. Solo puede haber una abierta.
    pub fn abrir(conn: &mut Connection, user_id: i64, fondo: Money) -> Result<i64> {
        if Self::abierta(conn)?.is_some() {
            return Err(DbError::Negocio(
                "Ya hay una caja abierta. Ciérrala antes de abrir otra.".into(),
            ));
        }
        if fondo.cents() < 0 {
            return Err(DbError::Negocio("El fondo no puede ser negativo".into()));
        }
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO cash_sessions (user_id, opening_float_cents, status)
             VALUES (?1, ?2, 'abierta')",
            params![user_id, fondo.cents()],
        )?;
        let id = tx.last_insert_rowid();
        if fondo.cents() > 0 {
            tx.execute(
                "INSERT INTO cash_movements (session_id, type, amount_cents, reason, user_id)
                 VALUES (?1, 'entrada', ?2, 'Fondo inicial', ?3)",
                params![id, fondo.cents(), user_id],
            )?;
        }
        tx.execute(
            "INSERT INTO audit_log (user_id, action, entity, entity_id)
             VALUES (?1, 'abrir_caja', 'cash_session', ?2)",
            params![user_id, id],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// La sesión abierta actual, si existe.
    pub fn abierta(conn: &Connection) -> Result<Option<CashSession>> {
        conn.query_row(
            "SELECT id, user_id, opening_float_cents, opened_at, closed_at, status
             FROM cash_sessions WHERE status = 'abierta' ORDER BY id DESC LIMIT 1",
            [],
            mapear_sesion,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn por_id(conn: &Connection, id: i64) -> Result<Option<CashSession>> {
        conn.query_row(
            "SELECT id, user_id, opening_float_cents, opened_at, closed_at, status
             FROM cash_sessions WHERE id = ?1",
            params![id],
            mapear_sesion,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Registra una entrada, un retiro o un gasto.
    pub fn movimiento(
        conn: &mut Connection,
        session_id: i64,
        tipo: MovimientoCaja,
        monto: Money,
        motivo: &str,
        user_id: i64,
    ) -> Result<()> {
        pos_core::validar_movimiento(tipo, monto, motivo)?;
        let sesion = Self::por_id(conn, session_id)?
            .ok_or(DbError::Negocio("No existe esa sesión de caja".into()))?;
        if !sesion.abierta() {
            return Err(DbError::Negocio("La caja ya está cerrada".into()));
        }

        conn.execute(
            "INSERT INTO cash_movements (session_id, type, amount_cents, reason, user_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session_id,
                tipo_str(tipo),
                monto.cents(),
                motivo.trim(),
                user_id
            ],
        )?;
        conn.execute(
            "INSERT INTO audit_log (user_id, action, entity, entity_id, payload_json)
             VALUES (?1, 'movimiento_caja', 'cash_session', ?2, ?3)",
            params![
                user_id,
                session_id,
                format!("{}|{}|{}", tipo_str(tipo), monto.cents(), motivo.trim())
            ],
        )?;
        Ok(())
    }

    /// Cuánto dinero *debería* haber en el cajón ahora mismo.
    pub fn corte_actual(conn: &Connection, session_id: i64) -> Result<CorteCaja> {
        let sesion = Self::por_id(conn, session_id)?
            .ok_or(DbError::Negocio("No existe esa sesión de caja".into()))?;

        let ventas_efectivo: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(p.amount_cents), 0)
                 FROM payments p
                 JOIN sales s ON s.id = p.sale_id
                 WHERE s.cash_session_id = ?1 AND s.status = 'completada'
                   AND p.method = 'efectivo'",
                params![session_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let mut corte = CorteCaja {
            fondo_inicial: sesion.fondo(),
            ventas_efectivo: Money::from_cents(ventas_efectivo),
            entradas: Money::ZERO,
            retiros: Money::ZERO,
            gastos: Money::ZERO,
        };

        let mut stmt = conn.prepare(
            "SELECT type, COALESCE(SUM(amount_cents), 0)
             FROM cash_movements WHERE session_id = ?1 AND reason <> 'Fondo inicial'
             GROUP BY type",
        )?;
        let filas = stmt
            .query_map(params![session_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for (tipo, monto) in filas {
            match tipo.as_str() {
                "entrada" => corte.entradas = Money::from_cents(monto),
                "retiro" => corte.retiros = Money::from_cents(monto),
                "gasto" => corte.gastos = Money::from_cents(monto),
                _ => {}
            }
        }
        Ok(corte)
    }

    /// Cierra la caja: compara lo esperado contra el arqueo y guarda la
    /// diferencia (aunque duela).
    pub fn cerrar(
        conn: &mut Connection,
        session_id: i64,
        arqueo: &Arqueo,
        user_id: i64,
    ) -> Result<Cierre> {
        let sesion = Self::por_id(conn, session_id)?
            .ok_or(DbError::Negocio("No existe esa sesión de caja".into()))?;
        if !sesion.abierta() {
            return Err(DbError::Negocio("La caja ya está cerrada".into()));
        }

        let corte = Self::corte_actual(conn, session_id)?;
        let contado = arqueo.total();
        let cierre = corte.cerrar(contado);

        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE cash_sessions
             SET status = 'cerrada', closed_at = datetime('now'), closed_by = ?1,
                 expected_cents = ?2, counted_cents = ?3, difference_cents = ?4
             WHERE id = ?5",
            params![
                user_id,
                cierre.esperado.cents(),
                cierre.contado.cents(),
                cierre.diferencia.cents(),
                session_id
            ],
        )?;
        tx.execute(
            "INSERT INTO audit_log (user_id, action, entity, entity_id, payload_json)
             VALUES (?1, 'cerrar_caja', 'cash_session', ?2, ?3)",
            params![
                user_id,
                session_id,
                format!(
                    "esperado={} contado={} diferencia={}",
                    cierre.esperado.cents(),
                    cierre.contado.cents(),
                    cierre.diferencia.cents()
                )
            ],
        )?;
        tx.commit()?;
        Ok(cierre)
    }

    /// Resumen de las ventas del turno (para el reporte de cierre).
    pub fn ventas_de_sesion(conn: &Connection, session_id: i64) -> Result<(i64, i64)> {
        let fila = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(total_cents), 0)
             FROM sales WHERE cash_session_id = ?1 AND status = 'completada'",
            params![session_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )?;
        Ok(fila)
    }

    /// Últimas sesiones cerradas, para ver el historial de diferencias.
    pub fn historial(conn: &Connection, limite: usize) -> Result<Vec<(String, i64, i64, i64)>> {
        let mut stmt = conn.prepare(
            "SELECT opened_at, COALESCE(expected_cents,0), COALESCE(counted_cents,0),
                    COALESCE(difference_cents,0)
             FROM cash_sessions WHERE status = 'cerrada'
             ORDER BY id DESC LIMIT ?1",
        )?;
        let filas = stmt
            .query_map(params![limite as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }
}

fn mapear_sesion(row: &rusqlite::Row<'_>) -> rusqlite::Result<CashSession> {
    let estado: String = row.get(5)?;
    Ok(CashSession {
        id: row.get(0)?,
        user_id: row.get(1)?,
        opening_float_cents: row.get(2)?,
        opened_at: row.get(3)?,
        closed_at: row.get(4)?,
        status: if estado == "abierta" {
            EstadoCaja::Abierta
        } else {
            EstadoCaja::Cerrada
        },
    })
}

fn tipo_str(t: MovimientoCaja) -> &'static str {
    match t {
        MovimientoCaja::Entrada => "entrada",
        MovimientoCaja::Retiro => "retiro",
        MovimientoCaja::Gasto => "gasto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::products::{NewProduct, Products};
    use crate::sales::Sales;
    use crate::Database;
    use pos_core::{Payment, PaymentMethod, Quantity, Sale, SaleLine, TaxRate};

    fn alta(db: &mut Database, sku: &str, precio: i64) -> i64 {
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

    /// Vende `qty` piezas y paga en el método indicado, dentro de la sesión.
    fn vender(
        db: &mut Database,
        product_id: i64,
        sku: &str,
        precio: i64,
        qty: i64,
        metodo: PaymentMethod,
        sesion: Option<i64>,
    ) -> i64 {
        let mut v = Sale::new("");
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
        v.add_payment(Payment::new(metodo, Money::from_cents(precio * qty)));
        Sales::save(db.conn_mut(), &mut v, 1, sesion).unwrap()
    }

    #[test]
    fn se_abre_la_caja_con_un_fondo_y_se_encuentra() {
        let mut db = Database::open_in_memory().unwrap();
        let id = Cash::abrir(db.conn_mut(), 1, Money::from_cents(50_000)).unwrap();
        let sesion = Cash::abierta(db.conn()).unwrap().unwrap();
        assert_eq!(sesion.id, id);
        assert!(sesion.abierta());
        assert_eq!(sesion.fondo(), Money::from_cents(50_000));
    }

    #[test]
    fn no_se_pueden_abrir_dos_cajas_a_la_vez() {
        let mut db = Database::open_in_memory().unwrap();
        Cash::abrir(db.conn_mut(), 1, Money::from_cents(10_000)).unwrap();
        assert!(Cash::abrir(db.conn_mut(), 1, Money::from_cents(10_000)).is_err());
    }

    #[test]
    fn el_efectivo_esperado_reune_ventas_retiros_y_gastos() {
        let mut db = Database::open_in_memory().unwrap();
        let sesion = Cash::abrir(db.conn_mut(), 1, Money::from_cents(50_000)).unwrap();
        let a = alta(&mut db, "A", 1000);

        vender(
            &mut db,
            a,
            "A",
            1000,
            10,
            PaymentMethod::Efectivo,
            Some(sesion),
        ); // +$100
        vender(
            &mut db,
            a,
            "A",
            1000,
            5,
            PaymentMethod::Tarjeta,
            Some(sesion),
        ); // no es efectivo

        let corte = Cash::corte_actual(db.conn(), sesion).unwrap();
        assert_eq!(corte.fondo_inicial, Money::from_cents(50_000));
        assert_eq!(corte.ventas_efectivo, Money::from_cents(10_000));
        assert_eq!(corte.esperado(), Money::from_cents(60_000));

        Cash::movimiento(
            db.conn_mut(),
            sesion,
            MovimientoCaja::Retiro,
            Money::from_cents(20_000),
            "Al banco",
            1,
        )
        .unwrap();
        Cash::movimiento(
            db.conn_mut(),
            sesion,
            MovimientoCaja::Gasto,
            Money::from_cents(5_000),
            "Papelería",
            1,
        )
        .unwrap();

        let corte = Cash::corte_actual(db.conn(), sesion).unwrap();
        assert_eq!(corte.esperado(), Money::from_cents(35_000));
    }

    #[test]
    fn el_cambio_entregado_no_infla_el_efectivo() {
        // El cliente paga con $200 una venta de $150: entran 200 y salen 50,
        // así que en el cajón quedan 150, no 200.
        let mut db = Database::open_in_memory().unwrap();
        let sesion = Cash::abrir(db.conn_mut(), 1, Money::ZERO).unwrap();
        let a = alta(&mut db, "A", 5000);

        let mut v = Sale::new("");
        v.add_line(SaleLine {
            product_id: Some(a),
            sku: "A".into(),
            description: "A".into(),
            qty: Quantity::from_units(3),
            unit_price: Money::from_cents(5000),
            discount: Money::ZERO,
            tax: TaxRate::IVA_16,
            unit_cost: Money::from_cents(2500),
        })
        .unwrap();
        let mut pago = Payment::new(PaymentMethod::Efectivo, Money::from_cents(15_000));
        pago.received = Money::from_cents(20_000);
        v.add_payment(pago);
        Sales::save(db.conn_mut(), &mut v, 1, Some(sesion)).unwrap();

        let corte = Cash::corte_actual(db.conn(), sesion).unwrap();
        assert_eq!(corte.ventas_efectivo, Money::from_cents(15_000));
    }

    #[test]
    fn el_cierre_guarda_la_diferencia_y_deja_la_caja_cerrada() {
        let mut db = Database::open_in_memory().unwrap();
        let sesion = Cash::abrir(db.conn_mut(), 1, Money::from_cents(50_000)).unwrap();
        let a = alta(&mut db, "A", 1000);
        vender(
            &mut db,
            a,
            "A",
            1000,
            10,
            PaymentMethod::Efectivo,
            Some(sesion),
        );

        // Esperado $600. El cajero cuenta $580: faltan $20.
        let mut arqueo = Arqueo::new();
        // 2 x $200 + 1 x $100 + 1 x $50 + 1 x $20 + 1 x $10 = $580
        arqueo.set_piezas(20_000, 2);
        arqueo.set_piezas(10_000, 1);
        arqueo.set_piezas(5_000, 1);
        arqueo.set_piezas(2_000, 1);
        arqueo.set_piezas(1_000, 1);
        assert_eq!(arqueo.total(), Money::from_cents(58_000));

        let cierre = Cash::cerrar(db.conn_mut(), sesion, &arqueo, 1).unwrap();
        assert_eq!(cierre.esperado, Money::from_cents(60_000));
        assert_eq!(cierre.contado, Money::from_cents(58_000));
        assert_eq!(cierre.faltante(), Money::from_cents(2_000));

        assert!(Cash::abierta(db.conn()).unwrap().is_none());
        // Cerrada dos veces: error.
        assert!(Cash::cerrar(db.conn_mut(), sesion, &arqueo, 1).is_err());
    }

    #[test]
    fn un_arqueo_exacto_cuadra_al_centavo() {
        let mut db = Database::open_in_memory().unwrap();
        let sesion = Cash::abrir(db.conn_mut(), 1, Money::from_cents(50_000)).unwrap();
        let a = alta(&mut db, "A", 1000);
        vender(
            &mut db,
            a,
            "A",
            1000,
            10,
            PaymentMethod::Efectivo,
            Some(sesion),
        );

        let mut arqueo = Arqueo::new();
        arqueo.set_piezas(10_000, 6); // 6 x $100 = $600
        let cierre = Cash::cerrar(db.conn_mut(), sesion, &arqueo, 1).unwrap();
        assert!(cierre.cuadra());
        assert_eq!(cierre.diferencia, Money::ZERO);
    }

    #[test]
    fn las_ventas_de_otra_sesion_no_se_cuelan_en_el_corte() {
        let mut db = Database::open_in_memory().unwrap();
        let a = alta(&mut db, "A", 1000);
        vender(&mut db, a, "A", 1000, 7, PaymentMethod::Efectivo, None); // sin sesión

        let sesion = Cash::abrir(db.conn_mut(), 1, Money::ZERO).unwrap();
        vender(
            &mut db,
            a,
            "A",
            1000,
            3,
            PaymentMethod::Efectivo,
            Some(sesion),
        );

        let corte = Cash::corte_actual(db.conn(), sesion).unwrap();
        assert_eq!(corte.ventas_efectivo, Money::from_cents(3_000));
    }

    #[test]
    fn los_retiros_exigen_motivo_y_la_caja_cerrada_no_acepta_movimientos() {
        let mut db = Database::open_in_memory().unwrap();
        let sesion = Cash::abrir(db.conn_mut(), 1, Money::from_cents(10_000)).unwrap();
        assert!(Cash::movimiento(
            db.conn_mut(),
            sesion,
            MovimientoCaja::Retiro,
            Money::from_cents(100),
            "",
            1
        )
        .is_err());

        let mut arqueo = Arqueo::new();
        arqueo.set_piezas(10_000, 1);
        Cash::cerrar(db.conn_mut(), sesion, &arqueo, 1).unwrap();
        assert!(Cash::movimiento(
            db.conn_mut(),
            sesion,
            MovimientoCaja::Retiro,
            Money::from_cents(100),
            "Tarde",
            1
        )
        .is_err());
    }

    #[test]
    fn el_resumen_del_turno_cuenta_tickets_e_ingresos() {
        let mut db = Database::open_in_memory().unwrap();
        let sesion = Cash::abrir(db.conn_mut(), 1, Money::ZERO).unwrap();
        let a = alta(&mut db, "A", 1000);
        vender(
            &mut db,
            a,
            "A",
            1000,
            2,
            PaymentMethod::Efectivo,
            Some(sesion),
        );
        vender(
            &mut db,
            a,
            "A",
            1000,
            3,
            PaymentMethod::Tarjeta,
            Some(sesion),
        );

        let (tickets, total) = Cash::ventas_de_sesion(db.conn(), sesion).unwrap();
        assert_eq!(tickets, 2);
        assert_eq!(total, 5_000);
    }
}
