//! Ventas: guardado atómico, consultas y reportes.
//!
//! `save()` es **todo o nada**: venta, líneas, pagos, movimientos de inventario
//! y la cola de sincronización se escriben en una sola transacción. Si se va la
//! luz en el milisegundo equivocado, no queda nada a medias.

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::clientes;
use crate::products::adjust_stock_tx;
use crate::{DbError, Result};
use pos_core::{PaymentMethod, Sale, SaleStatus};

/// La base guarda **siempre en UTC** (así un respaldo no cambia de significado
/// según quién lo abra). Para preguntar "¿de qué día es esta venta?" hay que
/// pasarla a hora de la Ciudad de México, y eso se hace **aquí y nada más aquí**.
///
/// Antes cada consulta lo hacía distinto: unas usaban `date('now','localtime')`
/// (que depende del reloj de la computadora) y otras comparaban contra la fecha
/// CDMX calculada en Rust. Resultado: en la tienda, al caer la tarde, la fecha
/// UTC ya era mañana y **los reportes del día salían vacíos**. Con un solo
/// criterio, eso no puede volver a pasar.
pub const MOD_LOCAL: &str = "'-6 hours'";

/// `date(s.created_at, '-6 hours')`: el día civil de una venta en México.
pub fn dia_local(columna: &str) -> String {
    format!("date({columna}, {MOD_LOCAL})")
}

/// `datetime('now', '-6 hours')`: ahora, en hora de México.
pub fn ahora_local() -> String {
    format!("datetime('now', {MOD_LOCAL})")
}

#[derive(Clone, Debug, PartialEq)]
pub struct SaleRecord {
    pub id: i64,
    pub folio: String,
    pub user_id: Option<i64>,
    pub customer_id: Option<i64>,
    pub subtotal_cents: i64,
    pub discount_cents: i64,
    pub tax_cents: i64,
    pub total_cents: i64,
    pub status: String,
    pub fiscal_status: String,
    pub created_at: String,
    pub items: Vec<SaleItemRow>,
    pub payments: Vec<PaymentRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SaleItemRow {
    pub sku: String,
    pub description: String,
    pub qty_millis: i64,
    pub unit_price_cents: i64,
    pub discount_cents: i64,
    pub tax_bp: i32,
    pub tax_cents: i64,
    pub cost_cents_at_sale: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaymentRow {
    pub method: String,
    pub amount_cents: i64,
    pub received_cents: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DayTotals {
    pub count: i64,
    pub subtotal_cents: i64,
    pub tax_cents: i64,
    pub discount_cents: i64,
    pub total_cents: i64,
    pub efectivo_cents: i64,
    pub tarjeta_cents: i64,
    pub transferencia_cents: i64,
    pub otros_cents: i64,
    /// Costo histórico de lo vendido hoy (congelado en la línea).
    pub costo_cents: i64,
    /// Vendido menos costo. Puede ser negativo si se vendió bajo costo.
    pub utilidad_cents: i64,
}

/// Margen de un producto en el día.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UtilidadProducto {
    pub sku: String,
    pub nombre: String,
    pub cantidad_millis: i64,
    pub vendido_cents: i64,
    pub costo_cents: i64,
    pub utilidad_cents: i64,
}

/// Cuánto vendió cada cajero en un día.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CajeroTotal {
    pub user_id: Option<i64>,
    pub nombre: String,
    pub tickets: i64,
    pub vendido_cents: i64,
    pub devuelto_cents: i64,
}

/// Una venta vista como renglón de reporte.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilaVenta {
    pub folio: String,
    pub fecha: String,
    pub cajero: String,
    pub subtotal_cents: i64,
    pub descuento_cents: i64,
    pub impuesto_cents: i64,
    pub total_cents: i64,
    pub estado: String,
    pub pagos: String,
    pub cambio_cents: i64,
}

/// ¿Esta venta se factura a nombre de alguien, o va a la global del día?
///
/// Se decide al cobrar, con los datos del cliente, para que la venta quede
/// marcada desde el principio y la factura global la deje fuera.
fn requiere_factura(conn: &Transaction, cliente_id: Option<i64>) -> bool {
    match cliente_id {
        Some(id) => clientes::datos_fiscales_completos(conn, id),
        None => false,
    }
}

pub struct Sales;

impl Sales {
    /// Guarda la venta completa. Devuelve el id y actualiza `sale.folio`.
    pub fn save(
        conn: &mut Connection,
        sale: &mut Sale,
        user_id: i64,
        session_id: Option<i64>,
    ) -> Result<i64> {
        if sale.is_empty() {
            return Err(DbError::Negocio("La venta no tiene productos".into()));
        }
        if !sale.is_fully_paid() {
            return Err(DbError::Negocio(format!(
                "Falta por cubrir {}",
                sale.balance_due()
            )));
        }

        // --- fiado: valida que haya cliente y crédito disponible -----------
        let fiado: i64 = sale
            .payments
            .iter()
            .filter(|p| p.method == pos_core::PaymentMethod::Credito)
            .map(|p| p.amount.cents())
            .sum();
        if fiado > 0 {
            let cid = sale.customer_id.ok_or_else(|| {
                DbError::Negocio("Una venta a crédito necesita un cliente".into())
            })?;
            let (limite, saldo): (i64, i64) = conn.query_row(
                "SELECT credit_limit_cents, balance_cents FROM customers WHERE id = ?1",
                params![cid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if saldo + fiado > limite {
                return Err(DbError::Negocio(format!(
                    "Crédito insuficiente: disponible ${:.2}, quiere fiar ${:.2}",
                    (limite - saldo) as f64 / 100.0,
                    fiado as f64 / 100.0
                )));
            }
        }

        // Congelamos los totales una sola vez: la BD, el ticket y el reporte
        // usarán exactamente los mismos números.
        let desglose = sale.tax_breakdown();
        let subtotal = sale.subtotal();
        let impuestos = sale.tax_total();
        let total = sale.total();
        let descuento = sale.discount_total();

        let tx = conn.transaction()?;
        let folio = if sale.folio.trim().is_empty() {
            next_folio(&tx)?
        } else {
            sale.folio.clone()
        };

        tx.execute(
            "INSERT INTO sales
             (folio, cash_session_id, user_id, customer_id,
              subtotal_cents, discount_cents, tax_cents, total_cents,
              status, fiscal_status, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'completada', ?10, ?9)",
            params![
                folio,
                session_id,
                user_id,
                sale.customer_id,
                subtotal.cents(),
                descuento.cents(),
                impuestos.cents(),
                total.cents(),
                sale.note,
                // Si la venta es a nombre de alguien con RFC, espera su propia
                // factura: meterla en la global del día haría que el SAT viera
                // el mismo ingreso dos veces.
                if requiere_factura(&tx, sale.customer_id) {
                    "requerida"
                } else {
                    "no_requerido"
                },
            ],
        )?;
        let sale_id = tx.last_insert_rowid();

        for (i, linea) in sale.lines.iter().enumerate() {
            let (_, iva_linea) = desglose[i];
            tx.execute(
                "INSERT INTO sale_items
                 (sale_id, product_id, sku, description, qty_millis, unit_price_cents,
                  discount_cents, tax_bp, tax_cents, cost_cents_at_sale)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    sale_id,
                    linea.product_id,
                    linea.sku,
                    linea.description,
                    linea.qty.millis(),
                    linea.unit_price.cents(),
                    linea.discount.cents(),
                    linea.tax.basis_points,
                    iva_linea.cents(),
                    linea.unit_cost.cents(),
                ],
            )?;

            if let Some(product_id) = linea.product_id {
                adjust_stock_tx(
                    &tx,
                    product_id,
                    -linea.qty.millis(),
                    "venta",
                    user_id,
                    Some(("sale", sale_id)),
                    None,
                )?;
            }
        }

        for pago in &sale.payments {
            // El fiado aumenta el saldo del cliente en la misma transacción.
            if pago.method == pos_core::PaymentMethod::Credito {
                let cid = sale.customer_id.unwrap_or_default();
                tx.execute(
                    "UPDATE customers SET balance_cents = balance_cents + ?1 WHERE id = ?2",
                    params![pago.amount.cents(), cid],
                )?;
            }
            tx.execute(
                "INSERT INTO payments (sale_id, method, amount_cents, received_cents, reference)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    sale_id,
                    method_str(pago.method),
                    pago.amount.cents(),
                    pago.received.cents(),
                    pago.reference,
                ],
            )?;
        }

        // En la Fase 6 esto alimenta el servidor LAN; en la Fase 5, al PAC.
        tx.execute(
            "INSERT INTO outbox (entity, entity_id, op) VALUES ('sale', ?1, 'created')",
            params![sale_id],
        )?;
        // Con folio, total y forma de pago: una bitácora que dice sólo "venta"
        // no sirve cuando estás buscando un ticket en particular.
        tx.execute(
            "INSERT INTO audit_log (user_id, action, entity, entity_id, payload_json)
             VALUES (?1, 'venta', 'sale', ?2, ?3)",
            params![
                user_id,
                sale_id,
                format!(
                    "Folio {} · {} · ${:.2}{}",
                    folio,
                    Self::resumen_lineas(sale),
                    sale.total().cents() as f64 / 100.0,
                    Self::formas_pago(sale)
                )
            ],
        )?;

        tx.commit()?;
        sale.id = Some(sale_id);
        sale.folio = folio;
        sale.status = SaleStatus::Completada;
        Ok(sale_id)
    }

    pub fn by_id(conn: &Connection, id: i64) -> Result<Option<SaleRecord>> {
        // La fecha sale en hora de México: el ticket y la lista de ventas la
        // muestran tal cual, y así un ticket de las 8 de la noche no aparece
        // fechado al día siguiente.
        let fila = conn
            .query_row(
                &format!(
                    "SELECT id, folio, user_id, customer_id, subtotal_cents, discount_cents,
                        tax_cents, total_cents, status, fiscal_status,
                        datetime(created_at, {MOD_LOCAL})
                 FROM sales WHERE id = ?1"
                ),
                params![id],
                |r| {
                    Ok(SaleRecord {
                        id: r.get(0)?,
                        folio: r.get(1)?,
                        user_id: r.get(2)?,
                        customer_id: r.get(3)?,
                        subtotal_cents: r.get(4)?,
                        discount_cents: r.get(5)?,
                        tax_cents: r.get(6)?,
                        total_cents: r.get(7)?,
                        status: r.get(8)?,
                        fiscal_status: r.get(9)?,
                        created_at: r.get(10)?,
                        items: Vec::new(),
                        payments: Vec::new(),
                    })
                },
            )
            .optional()?;

        let Some(mut venta) = fila else {
            return Ok(None);
        };
        venta.items = items_of(conn, venta.id)?;
        venta.payments = payments_of(conn, venta.id)?;
        Ok(Some(venta))
    }

    pub fn by_folio(conn: &Connection, folio: &str) -> Result<Option<SaleRecord>> {
        let id: Option<i64> = {
            let mut stmt = conn.prepare_cached("SELECT id FROM sales WHERE folio = ?1")?;
            stmt.query_row(params![folio.trim()], |r| r.get(0))
                .optional()?
        };
        match id {
            Some(id) => Self::by_id(conn, id),
            None => Ok(None),
        }
    }

    pub fn count(conn: &Connection) -> Result<i64> {
        Ok(conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?)
    }

    /// Últimas ventas: para la pantalla de devoluciones y reimpresión.
    pub fn recent(conn: &Connection, limit: usize) -> Result<Vec<SaleRecord>> {
        let mut stmt = conn.prepare("SELECT id FROM sales ORDER BY id DESC LIMIT ?1")?;
        let ids = stmt
            .query_map(params![limit as i64], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(v) = Self::by_id(conn, id)? {
                out.push(v);
            }
        }
        Ok(out)
    }

    /// Cancela/devuelve una venta y repone el inventario.
    pub fn cancel(conn: &mut Connection, sale_id: i64, user_id: i64) -> Result<()> {
        let estado: String = conn.query_row(
            "SELECT status FROM sales WHERE id = ?1",
            params![sale_id],
            |r| r.get(0),
        )?;
        if estado != "completada" {
            return Err(DbError::Negocio("Esa venta ya estaba cancelada".into()));
        }

        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE sales SET status = 'cancelada' WHERE id = ?1",
            params![sale_id],
        )?;

        let mut stmt =
            tx.prepare("SELECT product_id, qty_millis FROM sale_items WHERE sale_id = ?1")?;
        let renglones = stmt
            .query_map(params![sale_id], |r| {
                Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);

        for (product_id, qty) in renglones {
            if let Some(pid) = product_id {
                adjust_stock_tx(
                    &tx,
                    pid,
                    qty,
                    "devolucion",
                    user_id,
                    Some(("sale", sale_id)),
                    None,
                )?;
            }
        }
        tx.execute(
            "INSERT INTO audit_log (user_id, action, entity, entity_id)
             VALUES (?1, 'cancelacion', 'sale', ?2)",
            params![user_id, sale_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// "7501 x2, 7503 x1": para poder buscar una venta por producto.
    fn resumen_lineas(venta: &Sale) -> String {
        venta
            .lines
            .iter()
            .map(|l| {
                let qty = l.qty.millis() as f64 / 1000.0;
                format!("{} x{}", l.sku, (qty * 100.0).round() / 100.0)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// " · efectivo, tarjeta": para el detalle de la bitácora.
    fn formas_pago(venta: &Sale) -> String {
        if venta.payments.is_empty() {
            return String::new();
        }
        let mut formas: Vec<&str> = Vec::new();
        for pago in &venta.payments {
            let nombre = match pago.method {
                PaymentMethod::Efectivo => "efectivo",
                PaymentMethod::Tarjeta => "tarjeta",
                PaymentMethod::Transferencia => "transferencia",
                PaymentMethod::Vales => "vales",
                PaymentMethod::Credito => "crédito",
            };
            if !formas.contains(&nombre) {
                formas.push(nombre);
            }
        }
        format!(" · {}", formas.join(", "))
    }

    /// Totales del día: el dato que el dueño revisa antes de cerrar.
    pub fn totals_today(conn: &Connection) -> Result<DayTotals> {
        let mut t = DayTotals::default();
        // El "hoy" se calcula con el mismo criterio en las dos consultas.
        let hoy = dia_local("created_at");
        let ahora = format!("date({})", ahora_local());
        let fila = conn.query_row(
            &format!(
                "SELECT COUNT(*),
                    COALESCE(SUM(subtotal_cents),0),
                    COALESCE(SUM(tax_cents),0),
                    COALESCE(SUM(discount_cents),0),
                    COALESCE(SUM(total_cents),0)
             FROM sales
             WHERE {hoy} = {ahora} AND status = 'completada'"
            ),
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )?;
        t.count = fila.0;
        t.subtotal_cents = fila.1;
        t.tax_cents = fila.2;
        t.discount_cents = fila.3;
        t.total_cents = fila.4;

        let hoy_s = dia_local("s.created_at");
        let mut stmt = conn.prepare(&format!(
            "SELECT p.method, COALESCE(SUM(p.amount_cents),0)
             FROM payments p
             JOIN sales s ON s.id = p.sale_id
             WHERE {hoy_s} = {ahora} AND s.status = 'completada'
             GROUP BY p.method"
        ))?;
        let filas = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (metodo, monto) in filas {
            match metodo.as_str() {
                "efectivo" => t.efectivo_cents = monto,
                "tarjeta" => t.tarjeta_cents = monto,
                "transferencia" => t.transferencia_cents = monto,
                _ => t.otros_cents += monto,
            }
        }

        let hoy_i = dia_local("s.created_at");
        let fila_u = conn.query_row(
            &format!(
                "SELECT
                    COALESCE(SUM(i.unit_price_cents * i.qty_millis / 1000 - i.discount_cents), 0),
                    COALESCE(SUM(i.cost_cents_at_sale * i.qty_millis / 1000), 0)
                 FROM sale_items i
                 JOIN sales s ON s.id = i.sale_id
                 WHERE {hoy_i} = {ahora} AND s.status = 'completada'"
            ),
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )?;
        t.costo_cents = fila_u.1;
        t.utilidad_cents = fila_u.0 - fila_u.1;
        Ok(t)
    }

    /// Utilidad por producto del día: el margen real, con el costo de entonces.
    pub fn utilidad_hoy(conn: &Connection, limite: usize) -> Result<Vec<UtilidadProducto>> {
        let hoy = dia_local("s.created_at");
        let ahora = format!("date({})", ahora_local());
        let mut stmt = conn.prepare(&format!(
            "SELECT i.sku, i.description,
                    SUM(i.qty_millis),
                    SUM(i.unit_price_cents * i.qty_millis / 1000 - i.discount_cents),
                    SUM(i.cost_cents_at_sale * i.qty_millis / 1000)
             FROM sale_items i
             JOIN sales s ON s.id = i.sale_id
             WHERE {hoy} = {ahora} AND s.status = 'completada'
             GROUP BY i.sku
             ORDER BY (SUM(i.unit_price_cents * i.qty_millis / 1000 - i.discount_cents)
                     - SUM(i.cost_cents_at_sale * i.qty_millis / 1000)) DESC
             LIMIT ?1"
        ))?;
        let filas = stmt
            .query_map(params![limite as i64], |r| {
                let vendido: i64 = r.get(3)?;
                let costo: i64 = r.get(4)?;
                Ok(UtilidadProducto {
                    sku: r.get(0)?,
                    nombre: r.get(1)?,
                    cantidad_millis: r.get(2)?,
                    vendido_cents: vendido,
                    costo_cents: costo,
                    utilidad_cents: vendido - costo,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    /// Productos más vendidos (últimos 30 días): para decidir qué reabastecer.
    /// Ventas por cajero del día: quién vendió cuánto.
    pub fn por_cajero(conn: &Connection, fecha: &str) -> Result<Vec<CajeroTotal>> {
        let dia = dia_local("s.created_at");
        let mut stmt = conn.prepare(&format!(
            "SELECT u.id, COALESCE(u.display_name, u.username, 'Sin usuario'),
                    COUNT(DISTINCT s.id),
                    COALESCE(SUM(CASE WHEN s.status = 'completada' THEN s.total_cents ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN s.status = 'devuelta' THEN s.total_cents ELSE 0 END), 0)
             FROM sales s
             LEFT JOIN users u ON u.id = s.user_id
             WHERE {dia} = date(?1)
             GROUP BY u.id
             ORDER BY 4 DESC"
        ))?;
        let filas = stmt
            .query_map(params![fecha], |r| {
                Ok(CajeroTotal {
                    user_id: r.get(0)?,
                    nombre: r.get(1)?,
                    tickets: r.get(2)?,
                    vendido_cents: r.get(3)?,
                    devuelto_cents: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    /// Ventas de un rango de fechas, una fila por ticket: para exportar a CSV
    /// y para el reporte del contador.
    pub fn entre(
        conn: &Connection,
        desde: &str,
        hasta: &str,
        limite: usize,
    ) -> Result<Vec<FilaVenta>> {
        let dia = dia_local("s.created_at");
        let mut stmt = conn.prepare(&format!(
            "SELECT s.folio, datetime(s.created_at, {MOD_LOCAL}), COALESCE(u.display_name, u.username, ''),
                    s.subtotal_cents, s.discount_cents, s.tax_cents, s.total_cents,
                    s.status,
                    COALESCE((SELECT group_concat(p.method || ' ' || printf('%.2f', p.amount_cents / 100.0), ' + ')
                              FROM payments p WHERE p.sale_id = s.id), ''),
                    COALESCE((SELECT SUM(p.received_cents - p.amount_cents)
                              FROM payments p WHERE p.sale_id = s.id AND p.method = 'efectivo'), 0)
             FROM sales s
             LEFT JOIN users u ON u.id = s.user_id
             WHERE {dia} BETWEEN date(?1) AND date(?2)
             ORDER BY s.created_at DESC
             LIMIT ?3"
        ))?;
        let filas = stmt
            .query_map(params![desde, hasta, limite as i64], |r| {
                Ok(FilaVenta {
                    folio: r.get(0)?,
                    fecha: r.get(1)?,
                    cajero: r.get(2)?,
                    subtotal_cents: r.get(3)?,
                    descuento_cents: r.get(4)?,
                    impuesto_cents: r.get(5)?,
                    total_cents: r.get(6)?,
                    estado: r.get(7)?,
                    pagos: r.get(8)?,
                    cambio_cents: r.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    pub fn top_products(conn: &Connection, limit: usize) -> Result<Vec<(String, i64, i64)>> {
        let mut stmt = conn.prepare(
            "SELECT i.description, SUM(i.qty_millis), SUM(i.unit_price_cents * i.qty_millis / 1000)
             FROM sale_items i
             JOIN sales s ON s.id = i.sale_id
             WHERE s.status = 'completada' AND s.created_at >= datetime('now','-30 days')
             GROUP BY i.sku
             ORDER BY 3 DESC
             LIMIT ?1",
        )?;
        let filas = stmt
            .query_map(params![limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }
}

fn method_str(m: pos_core::PaymentMethod) -> &'static str {
    match m {
        pos_core::PaymentMethod::Efectivo => "efectivo",
        pos_core::PaymentMethod::Tarjeta => "tarjeta",
        pos_core::PaymentMethod::Transferencia => "transferencia",
        pos_core::PaymentMethod::Vales => "vales",
        pos_core::PaymentMethod::Credito => "credito",
    }
}

fn items_of(conn: &Connection, sale_id: i64) -> Result<Vec<SaleItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT sku, description, qty_millis, unit_price_cents, discount_cents,
                tax_bp, tax_cents, cost_cents_at_sale
         FROM sale_items WHERE sale_id = ?1 ORDER BY id",
    )?;
    let filas = stmt
        .query_map(params![sale_id], |r| {
            Ok(SaleItemRow {
                sku: r.get(0)?,
                description: r.get(1)?,
                qty_millis: r.get(2)?,
                unit_price_cents: r.get(3)?,
                discount_cents: r.get(4)?,
                tax_bp: r.get(5)?,
                tax_cents: r.get(6)?,
                cost_cents_at_sale: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(filas)
}

fn payments_of(conn: &Connection, sale_id: i64) -> Result<Vec<PaymentRow>> {
    let mut stmt = conn
        .prepare("SELECT method, amount_cents, received_cents FROM payments WHERE sale_id = ?1")?;
    let filas = stmt
        .query_map(params![sale_id], |r| {
            Ok(PaymentRow {
                method: r.get(0)?,
                amount_cents: r.get(1)?,
                received_cents: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(filas)
}

fn next_folio(tx: &Transaction<'_>) -> Result<String> {
    tx.execute(
        "UPDATE counters SET value = value + 1 WHERE name = 'folio'",
        [],
    )?;
    let valor: i64 = tx.query_row("SELECT value FROM counters WHERE name = 'folio'", [], |r| {
        r.get(0)
    })?;
    Ok(format!("{valor:06}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::products::{NewProduct, Products};
    use crate::Database;
    use pos_core::{Money, Payment, PaymentMethod, Quantity, SaleLine, TaxRate};

    fn alta(db: &mut Database, sku: &str, precio: i64, stock: i64) -> i64 {
        Products::insert(
            db.conn_mut(),
            &NewProduct {
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
            },
        )
        .unwrap()
    }

    /// Inserta una venta ya terminada con una fecha elegida a mano (en UTC,
    /// que es como se guardan). Sirve para probar el cambio de día.
    fn venta_en(db: &Database, folio: &str, cuando_utc: &str) {
        db.conn()
            .execute(
                "INSERT INTO sales (folio, subtotal_cents, discount_cents, tax_cents,
                                    total_cents, status, created_at)
                 VALUES (?1, 1000, 0, 160, 1000, 'completada', ?2)",
                rusqlite::params![folio, cuando_utc],
            )
            .unwrap();
    }

    #[test]
    fn una_venta_de_noche_cuenta_en_el_dia_de_la_tienda() {
        // Son las 8 de la noche en la tienda, pero el reloj UTC ya pasó a
        // mañana. Esa venta es de HOY para el cajero y para el corte.
        let db = Database::open_in_memory().unwrap();
        venta_en(&db, "000099", "2026-10-11 02:00:00"); // = 2026-10-10 20:00 CDMX

        assert_eq!(
            Sales::por_cajero(db.conn(), "2026-10-10").unwrap().len(),
            1,
            "la venta de la noche es del 10 en México"
        );
        assert_eq!(
            Sales::por_cajero(db.conn(), "2026-10-11").unwrap().len(),
            0,
            "no puede contar como del 11"
        );
        assert_eq!(
            Sales::entre(db.conn(), "2026-10-10", "2026-10-10", 100)
                .unwrap()
                .len(),
            1,
            "el rango del día también la debe incluir"
        );
        assert_eq!(
            Sales::entre(db.conn(), "2026-10-11", "2026-10-11", 100)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn una_venta_de_madrugada_tambien_cae_en_el_dia_correcto() {
        // El otro extremo: las 2 de la mañana del 10 en México son las 8 UTC
        // del 10. Ahí UTC y local sí coinciden.
        let db = Database::open_in_memory().unwrap();
        venta_en(&db, "000098", "2026-10-10 08:00:00");

        assert_eq!(Sales::por_cajero(db.conn(), "2026-10-10").unwrap().len(), 1);
        assert_eq!(Sales::por_cajero(db.conn(), "2026-10-09").unwrap().len(), 0);
    }

    #[test]
    fn el_reporte_por_cajero_y_el_csv_usan_el_mismo_criterio() {
        // Los dos caminos que alimentan los reportes tienen que estar de
        // acuerdo: si uno dice 10 y otro dice 11, el corte no cuadra.
        let db = Database::open_in_memory().unwrap();
        venta_en(&db, "000097", "2026-10-11 03:30:00"); // 21:30 del 10 en MX
        let por_cajero = Sales::por_cajero(db.conn(), "2026-10-10").unwrap().len();
        let entre = Sales::entre(db.conn(), "2026-10-10", "2026-10-10", 100)
            .unwrap()
            .len();
        assert_eq!(por_cajero, entre, "los dos reportes deben coincidir");
        assert_eq!(por_cajero, 1);
    }

    #[test]
    fn la_hora_que_se_muestra_es_la_de_mexico() {
        let db = Database::open_in_memory().unwrap();
        venta_en(&db, "000096", "2026-10-11 02:00:00");
        let venta = Sales::by_folio(db.conn(), "000096").unwrap().unwrap();
        assert!(
            venta.created_at.starts_with("2026-10-10 20:00"),
            "el ticket debe decir la hora de la tienda, no la UTC: {}",
            venta.created_at
        );
    }

    fn linea(product_id: i64, sku: &str, precio: i64, qty: i64) -> SaleLine {
        SaleLine {
            product_id: Some(product_id),
            sku: sku.into(),
            description: sku.into(),
            qty: Quantity::from_units(qty),
            unit_price: Money::from_cents(precio),
            discount: Money::ZERO,
            tax: TaxRate::IVA_16,
            unit_cost: Money::from_cents(precio / 2),
        }
    }

    fn venta_pagada(lineas: Vec<SaleLine>, total: i64) -> Sale {
        let mut v = Sale::new("");
        for l in lineas {
            v.add_line(l).unwrap();
        }
        v.add_payment(Payment::new(
            PaymentMethod::Efectivo,
            Money::from_cents(total),
        ));
        v
    }

    #[test]
    fn guarda_venta_y_genera_folio_consecutivo() {
        let mut db = Database::open_in_memory().unwrap();
        let id = alta(&mut db, "A", 1000, 100_000);
        let mut v = venta_pagada(vec![linea(id, "A", 1000, 2)], 2000);
        let sale_id = Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();
        assert_eq!(v.folio, "000001");

        // Reimprimir / devolver por folio.
        let misma = Sales::by_folio(db.conn(), "000001").unwrap().unwrap();
        assert_eq!(misma.id, sale_id);
        assert_eq!(misma.items.len(), 1);
        assert_eq!(misma.payments[0].method, "efectivo");
    }

    #[test]
    fn la_utilidad_usa_el_costo_de_entonces_no_el_de_ahora() {
        let mut db = Database::open_in_memory().unwrap();
        let id = alta(&mut db, "COCA", 1800, 100_000);
        let mut v = venta_pagada(vec![linea(id, "COCA", 1800, 2)], 3600);
        Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();
        // El costo de lista cambia después: el margen del día no se mueve.
        db.conn()
            .execute("UPDATE products SET cost_cents = 1 WHERE sku = 'COCA'", [])
            .unwrap();
        let t = Sales::totals_today(db.conn()).unwrap();
        assert_eq!(t.costo_cents, 1800, "2 × $9.00 de costo histórico");
        assert_eq!(t.utilidad_cents, 1800, "$36 vendido − $18 costo");
        let por = Sales::utilidad_hoy(db.conn(), 10).unwrap();
        assert_eq!(por.len(), 1);
        assert_eq!(por[0].sku, "COCA");
        assert_eq!(por[0].utilidad_cents, 1800);
    }

    #[test]
    fn los_totales_guardados_cuadran_con_el_nucleo() {
        let mut db = Database::open_in_memory().unwrap();
        let a = alta(&mut db, "A", 1850, 100_000);
        let b = alta(&mut db, "B", 2600, 100_000);
        let mut v = venta_pagada(vec![linea(a, "A", 1850, 3), linea(b, "B", 2600, 1)], 8150);
        v.apply_global_discount(Money::from_cents(500)).unwrap();
        let sale_id = Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();

        let guardada = Sales::by_id(db.conn(), sale_id).unwrap().unwrap();
        assert_eq!(guardada.total_cents, v.total().cents());
        assert_eq!(guardada.total_cents, 7650);
        assert_eq!(guardada.subtotal_cents, v.subtotal().cents());
        assert_eq!(guardada.tax_cents, v.tax_total().cents());
        // Suma de impuestos por línea = impuesto total de la venta.
        let suma_items: i64 = guardada.items.iter().map(|i| i.tax_cents).sum();
        assert_eq!(suma_items, guardada.tax_cents);
    }

    #[test]
    fn rechaza_ventas_sin_cubrir_el_total() {
        let mut db = Database::open_in_memory().unwrap();
        let a = alta(&mut db, "A", 1000, 100_000);
        let mut v = Sale::new("");
        v.add_line(linea(a, "A", 1000, 1)).unwrap();
        v.add_payment(Payment::new(
            PaymentMethod::Efectivo,
            Money::from_cents(500),
        ));
        assert!(Sales::save(db.conn_mut(), &mut v, 1, None).is_err());
        assert_eq!(Sales::count(db.conn()).unwrap(), 0);
        // El inventario quedó intacto.
        assert_eq!(
            Products::get(db.conn(), a).unwrap().unwrap().stock_millis,
            100_000
        );
    }

    #[test]
    fn pagos_mixtos_se_guardan_por_separado() {
        let mut db = Database::open_in_memory().unwrap();
        let a = alta(&mut db, "A", 1000, 100_000);
        let mut v = Sale::new("");
        v.add_line(linea(a, "A", 1000, 5)).unwrap();
        v.add_payment(Payment::new(
            PaymentMethod::Tarjeta,
            Money::from_cents(2000),
        ));
        let mut efectivo = Payment::new(PaymentMethod::Efectivo, Money::from_cents(3000));
        efectivo.received = Money::from_cents(5000);
        v.add_payment(efectivo);
        let id = Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();

        let guardada = Sales::by_id(db.conn(), id).unwrap().unwrap();
        assert_eq!(guardada.payments.len(), 2);
        assert_eq!(guardada.total_cents, 5000);
        assert_eq!(guardada.payments[1].received_cents, 5000); // cambio 2000
    }

    #[test]
    fn el_reporte_top_de_productos_ordena_por_importe() {
        let mut db = Database::open_in_memory().unwrap();
        let a = alta(&mut db, "A", 1000, 100_000);
        let b = alta(&mut db, "B", 5000, 100_000);
        let mut v1 = Sale::new("");
        v1.add_line(linea(a, "A", 1000, 10)).unwrap(); // 100.00
        v1.add_payment(Payment::new(
            PaymentMethod::Efectivo,
            Money::from_cents(10_000),
        ));
        Sales::save(db.conn_mut(), &mut v1, 1, None).unwrap();

        let mut v2 = Sale::new("");
        v2.add_line(linea(b, "B", 5000, 3)).unwrap(); // 150.00
        v2.add_payment(Payment::new(
            PaymentMethod::Efectivo,
            Money::from_cents(15_000),
        ));
        Sales::save(db.conn_mut(), &mut v2, 1, None).unwrap();

        let top = Sales::top_products(db.conn(), 5).unwrap();
        assert_eq!(top[0].0, "B");
    }
}
