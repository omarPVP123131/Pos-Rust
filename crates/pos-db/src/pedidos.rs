//! Pedidos a proveedor.
//!
//! En una tienda de barrio el flujo es corto: se anota lo que se pidió, llega
//! la caja, se cuenta y el anaquel sube. Nada de órdenes electrónicas ni de
//! varios almacenes. El inventario **sólo** se mueve al recibir, en la misma
//! transacción que marca las piezas, para que un apagón no deje medio pedido.

use rusqlite::{params, Connection, OptionalExtension};

use crate::products::{adjust_stock_tx, Products};
use crate::{DbError, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Proveedor {
    pub id: i64,
    pub name: String,
    pub phone: String,
    pub nota: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineaPedido {
    pub product_id: i64,
    pub sku: String,
    pub nombre: String,
    pub qty_pedida_millis: i64,
    pub qty_recibida_millis: i64,
    pub costo_cents: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pedido {
    pub id: i64,
    pub folio: String,
    pub proveedor_id: i64,
    pub proveedor: String,
    pub estado: String,
    pub nota: String,
    pub created_at: String,
    pub recibido_at: Option<String>,
    pub lineas: Vec<LineaPedido>,
}

impl Pedido {
    pub fn total_costo_cents(&self) -> i64 {
        self.lineas
            .iter()
            .map(|l| l.costo_cents.saturating_mul(l.qty_pedida_millis) / 1_000)
            .sum()
    }
}

/// Lo que llegó en esta entrega: producto y piezas (milésimas).
#[derive(Clone, Debug, PartialEq)]
pub struct Entrega {
    pub product_id: i64,
    pub qty_millis: i64,
}

pub fn listar_proveedores(conn: &Connection, termino: &str) -> Result<Vec<Proveedor>> {
    let like = format!("%{}%", termino.trim());
    let mut stmt = conn.prepare(
        "SELECT id, name, phone, nota, is_active
         FROM proveedores
         WHERE is_active = 1 AND (name LIKE ?1 COLLATE NOCASE OR phone LIKE ?1)
         ORDER BY name COLLATE NOCASE
         LIMIT 200",
    )?;
    let filas = stmt
        .query_map(params![like], |r| {
            Ok(Proveedor {
                id: r.get(0)?,
                name: r.get(1)?,
                phone: r.get(2)?,
                nota: r.get(3)?,
                is_active: r.get::<_, i32>(4)? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(filas)
}

pub fn guardar_proveedor(
    conn: &Connection,
    id: Option<i64>,
    name: &str,
    phone: &str,
    nota: &str,
) -> Result<i64> {
    let name = name.trim();
    if name.len() < 2 {
        return Err(DbError::Negocio(
            "El proveedor necesita un nombre (al menos 2 letras).".into(),
        ));
    }
    if let Some(id) = id {
        let n = conn.execute(
            "UPDATE proveedores SET name = ?1, phone = ?2, nota = ?3 WHERE id = ?4",
            params![name, phone.trim(), nota.trim(), id],
        )?;
        if n == 0 {
            return Err(DbError::Negocio("No encontré ese proveedor.".into()));
        }
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO proveedores (name, phone, nota) VALUES (?1, ?2, ?3)",
        params![name, phone.trim(), nota.trim()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn listar_pedidos(conn: &Connection, limite: usize) -> Result<Vec<Pedido>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.folio, p.proveedor_id, v.name, p.estado, p.nota,
                p.created_at, p.recibido_at
         FROM pedidos p
         JOIN proveedores v ON v.id = p.proveedor_id
         ORDER BY p.id DESC
         LIMIT ?1",
    )?;
    let mut filas = Vec::new();
    let mut q = stmt.query(params![limite.max(1) as i64])?;
    while let Some(r) = q.next()? {
        let id: i64 = r.get(0)?;
        filas.push(Pedido {
            id,
            folio: r.get(1)?,
            proveedor_id: r.get(2)?,
            proveedor: r.get(3)?,
            estado: r.get(4)?,
            nota: r.get(5)?,
            created_at: r.get(6)?,
            recibido_at: r.get(7)?,
            lineas: lineas_de(conn, id)?,
        });
    }
    Ok(filas)
}

pub fn ver(conn: &Connection, id: i64) -> Result<Pedido> {
    conn.query_row(
        "SELECT p.id, p.folio, p.proveedor_id, v.name, p.estado, p.nota,
                p.created_at, p.recibido_at
         FROM pedidos p
         JOIN proveedores v ON v.id = p.proveedor_id
         WHERE p.id = ?1",
        params![id],
        |r| {
            Ok(Pedido {
                id: r.get(0)?,
                folio: r.get(1)?,
                proveedor_id: r.get(2)?,
                proveedor: r.get(3)?,
                estado: r.get(4)?,
                nota: r.get(5)?,
                created_at: r.get(6)?,
                recibido_at: r.get(7)?,
                lineas: Vec::new(),
            })
        },
    )
    .optional()?
    .ok_or_else(|| DbError::Negocio("No encontré ese pedido.".into()))
    .and_then(|mut p| {
        p.lineas = lineas_de(conn, p.id)?;
        Ok(p)
    })
}

fn lineas_de(conn: &Connection, pedido_id: i64) -> Result<Vec<LineaPedido>> {
    let mut stmt = conn.prepare(
        "SELECT l.product_id, p.sku, p.name, l.qty_pedida_millis,
                l.qty_recibida_millis, l.costo_cents
         FROM pedido_lineas l
         JOIN products p ON p.id = l.product_id
         WHERE l.pedido_id = ?1
         ORDER BY p.name COLLATE NOCASE",
    )?;
    let filas = stmt
        .query_map(params![pedido_id], |r| {
            Ok(LineaPedido {
                product_id: r.get(0)?,
                sku: r.get(1)?,
                nombre: r.get(2)?,
                qty_pedida_millis: r.get(3)?,
                qty_recibida_millis: r.get(4)?,
                costo_cents: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(filas)
}

pub fn nuevo(conn: &mut Connection, proveedor_id: i64, nota: &str, user_id: i64) -> Result<Pedido> {
    let existe: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proveedores WHERE id = ?1 AND is_active = 1",
        params![proveedor_id],
        |r| r.get(0),
    )?;
    if existe == 0 {
        return Err(DbError::Negocio("No encontré ese proveedor.".into()));
    }
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO counters (name, value) VALUES ('pedido', 0)",
        [],
    )?;
    tx.execute(
        "UPDATE counters SET value = value + 1 WHERE name = 'pedido'",
        [],
    )?;
    let n: i64 = tx.query_row(
        "SELECT value FROM counters WHERE name = 'pedido'",
        [],
        |r| r.get(0),
    )?;
    let folio = format!("P-{n:06}");
    tx.execute(
        "INSERT INTO pedidos (folio, proveedor_id, estado, nota, creado_por)
         VALUES (?1, ?2, 'abierto', ?3, ?4)",
        params![folio, proveedor_id, nota.trim(), user_id],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    ver(conn, id)
}

/// Agrega (o suma) un producto al pedido abierto. `codigo` es SKU o barras.
pub fn agregar(
    conn: &mut Connection,
    pedido_id: i64,
    codigo: &str,
    qty_millis: i64,
    costo_cents: Option<i64>,
) -> Result<Pedido> {
    if qty_millis <= 0 {
        return Err(DbError::Negocio(
            "La cantidad pedida tiene que ser más que cero.".into(),
        ));
    }
    let pedido = ver(conn, pedido_id)?;
    if pedido.estado != "abierto" {
        return Err(DbError::Negocio(format!(
            "El pedido {} ya no se puede editar (está {}).",
            pedido.folio, pedido.estado
        )));
    }
    if pedido.lineas.iter().any(|l| l.qty_recibida_millis > 0) {
        return Err(DbError::Negocio(
            "Este pedido ya recibió mercancía. Si falta algo, abre otro.".into(),
        ));
    }
    let codigo = codigo.trim();
    let prod = Products::by_barcode(conn, codigo)?
        .or(Products::by_sku(conn, codigo)?)
        .ok_or_else(|| {
            DbError::Negocio(format!(
                "No encontré el producto '{codigo}' en el catálogo."
            ))
        })?;
    let costo = costo_cents.unwrap_or(prod.cost_cents).max(0);
    conn.execute(
        "INSERT INTO pedido_lineas (pedido_id, product_id, qty_pedida_millis, costo_cents)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(pedido_id, product_id) DO UPDATE SET
            qty_pedida_millis = qty_pedida_millis + excluded.qty_pedida_millis,
            costo_cents = excluded.costo_cents",
        params![pedido_id, prod.id, qty_millis, costo],
    )?;
    ver(conn, pedido_id)
}

pub fn quitar(conn: &mut Connection, pedido_id: i64, product_id: i64) -> Result<Pedido> {
    let pedido = ver(conn, pedido_id)?;
    if pedido.estado != "abierto" {
        return Err(DbError::Negocio(format!(
            "El pedido {} ya no se puede editar.",
            pedido.folio
        )));
    }
    if pedido
        .lineas
        .iter()
        .any(|l| l.product_id == product_id && l.qty_recibida_millis > 0)
    {
        return Err(DbError::Negocio(
            "Ese producto ya llegó: no se puede quitar del pedido.".into(),
        ));
    }
    conn.execute(
        "DELETE FROM pedido_lineas WHERE pedido_id = ?1 AND product_id = ?2",
        params![pedido_id, product_id],
    )?;
    ver(conn, pedido_id)
}

pub fn cancelar(conn: &mut Connection, pedido_id: i64) -> Result<Pedido> {
    let pedido = ver(conn, pedido_id)?;
    if pedido.estado != "abierto" {
        return Err(DbError::Negocio(format!(
            "El pedido {} no se puede cancelar (está {}).",
            pedido.folio, pedido.estado
        )));
    }
    if pedido.lineas.iter().any(|l| l.qty_recibida_millis > 0) {
        return Err(DbError::Negocio(
            "Ya llegó mercancía de este pedido. Lo que llegó se queda en el inventario.".into(),
        ));
    }
    conn.execute(
        "UPDATE pedidos SET estado = 'cancelado' WHERE id = ?1",
        params![pedido_id],
    )?;
    ver(conn, pedido_id)
}

/// Recibe mercancía: sube el inventario y anota lo que llegó.
///
/// Se puede recibir a medias (llegan 8 de 10) y completar después. Cuando
/// todas las líneas están cubiertas, el pedido pasa a `recibido`.
pub fn recibir(
    conn: &mut Connection,
    pedido_id: i64,
    entregas: &[Entrega],
    user_id: i64,
) -> Result<Pedido> {
    let pedido = ver(conn, pedido_id)?;
    if pedido.estado != "abierto" {
        return Err(DbError::Negocio(format!(
            "El pedido {} no está abierto.",
            pedido.folio
        )));
    }
    if pedido.lineas.is_empty() {
        return Err(DbError::Negocio("Ese pedido no tiene productos.".into()));
    }
    if entregas.iter().all(|e| e.qty_millis <= 0) {
        return Err(DbError::Negocio("No marcaste nada para recibir.".into()));
    }

    let tx = conn.transaction()?;
    for e in entregas {
        if e.qty_millis <= 0 {
            continue;
        }
        let (pedida, recibida, costo): (i64, i64, i64) = tx
            .query_row(
                "SELECT qty_pedida_millis, qty_recibida_millis, costo_cents
                 FROM pedido_lineas WHERE pedido_id = ?1 AND product_id = ?2",
                params![pedido_id, e.product_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| DbError::Negocio("Ese producto no está en el pedido.".into()))?;
        let queda = pedida - recibida;
        if e.qty_millis > queda {
            let nombre: String = tx
                .query_row(
                    "SELECT name FROM products WHERE id = ?1",
                    params![e.product_id],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| "el producto".into());
            return Err(DbError::Negocio(format!(
                "De '{nombre}' pediste {} y ya llegaron {}: no puedes recibir {} más.",
                pedida / 1000,
                recibida / 1000,
                e.qty_millis / 1000
            )));
        }
        adjust_stock_tx(
            &tx,
            e.product_id,
            e.qty_millis,
            "compra",
            user_id,
            Some(("pedido", pedido_id)),
            Some(costo),
        )?;
        tx.execute(
            "UPDATE pedido_lineas
             SET qty_recibida_millis = qty_recibida_millis + ?1
             WHERE pedido_id = ?2 AND product_id = ?3",
            params![e.qty_millis, pedido_id, e.product_id],
        )?;
        if costo > 0 {
            tx.execute(
                "UPDATE products SET cost_cents = ?1, updated_at = datetime('now')
                 WHERE id = ?2",
                params![costo, e.product_id],
            )?;
        }
    }

    let pendientes: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pedido_lineas
         WHERE pedido_id = ?1 AND qty_recibida_millis < qty_pedida_millis",
        params![pedido_id],
        |r| r.get(0),
    )?;
    if pendientes == 0 {
        tx.execute(
            "UPDATE pedidos SET estado = 'recibido', recibido_at = datetime('now')
             WHERE id = ?1",
            params![pedido_id],
        )?;
    }
    tx.commit()?;
    ver(conn, pedido_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::products::NewProduct;
    use crate::Database;

    fn tienda() -> Database {
        let mut db = Database::open_in_memory().unwrap();
        crate::products::Products::insert(
            db.conn_mut(),
            &NewProduct {
                sku: "7501".into(),
                name: "Coca-Cola 600 ml".into(),
                price_cents: 1800,
                cost_cents: 1200,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: 10_000,
                min_stock_millis: 0,
                track_inventory: true,
                barcodes: vec!["750100000".into()],
            },
        )
        .unwrap();
        db
    }

    #[test]
    fn pedir_y_recibir_sube_el_inventario() {
        let mut db = tienda();
        let pid = guardar_proveedor(db.conn(), None, "Bebidas del Valle", "555", "").unwrap();
        let pedido = nuevo(db.conn_mut(), pid, "", 1).unwrap();
        assert_eq!(pedido.folio, "P-000001");
        let pedido = agregar(db.conn_mut(), pedido.id, "7501", 5_000, Some(1100)).unwrap();
        assert_eq!(pedido.lineas.len(), 1);
        assert_eq!(pedido.lineas[0].qty_pedida_millis, 5_000);

        let antes = Products::by_sku(db.conn(), "7501").unwrap().unwrap();
        assert_eq!(antes.stock_millis, 10_000);

        let pedido = recibir(
            db.conn_mut(),
            pedido.id,
            &[Entrega {
                product_id: pedido.lineas[0].product_id,
                qty_millis: 5_000,
            }],
            1,
        )
        .unwrap();
        assert_eq!(pedido.estado, "recibido");
        let despues = Products::by_sku(db.conn(), "7501").unwrap().unwrap();
        assert_eq!(despues.stock_millis, 15_000);
        assert_eq!(despues.cost_cents, 1100, "el costo queda el de esta compra");
    }

    #[test]
    fn se_puede_recibir_a_medias_y_completar() {
        let mut db = tienda();
        let pid = guardar_proveedor(db.conn(), None, "Bimbo", "", "").unwrap();
        let pedido = nuevo(db.conn_mut(), pid, "", 1).unwrap();
        let prod = Products::by_sku(db.conn(), "7501").unwrap().unwrap();
        agregar(db.conn_mut(), pedido.id, "750100000", 10_000, None).unwrap();

        let p = recibir(
            db.conn_mut(),
            pedido.id,
            &[Entrega {
                product_id: prod.id,
                qty_millis: 4_000,
            }],
            1,
        )
        .unwrap();
        assert_eq!(p.estado, "abierto");
        assert_eq!(p.lineas[0].qty_recibida_millis, 4_000);
        assert_eq!(
            Products::by_sku(db.conn(), "7501")
                .unwrap()
                .unwrap()
                .stock_millis,
            14_000
        );

        let p = recibir(
            db.conn_mut(),
            pedido.id,
            &[Entrega {
                product_id: prod.id,
                qty_millis: 6_000,
            }],
            1,
        )
        .unwrap();
        assert_eq!(p.estado, "recibido");
        assert_eq!(
            Products::by_sku(db.conn(), "7501")
                .unwrap()
                .unwrap()
                .stock_millis,
            20_000
        );
    }

    #[test]
    fn no_se_cancela_si_ya_llego_mercancia() {
        let mut db = tienda();
        let pid = guardar_proveedor(db.conn(), None, "Acme", "", "").unwrap();
        let pedido = nuevo(db.conn_mut(), pid, "", 1).unwrap();
        let prod = Products::by_sku(db.conn(), "7501").unwrap().unwrap();
        agregar(db.conn_mut(), pedido.id, "7501", 2_000, None).unwrap();
        recibir(
            db.conn_mut(),
            pedido.id,
            &[Entrega {
                product_id: prod.id,
                qty_millis: 1_000,
            }],
            1,
        )
        .unwrap();
        let err = cancelar(db.conn_mut(), pedido.id).unwrap_err();
        assert!(err.to_string().contains("llegó"), "{err}");
    }

    #[test]
    fn no_se_recibe_de_mas() {
        let mut db = tienda();
        let pid = guardar_proveedor(db.conn(), None, "Acme", "", "").unwrap();
        let pedido = nuevo(db.conn_mut(), pid, "", 1).unwrap();
        let prod = Products::by_sku(db.conn(), "7501").unwrap().unwrap();
        agregar(db.conn_mut(), pedido.id, "7501", 2_000, None).unwrap();
        let err = recibir(
            db.conn_mut(),
            pedido.id,
            &[Entrega {
                product_id: prod.id,
                qty_millis: 3_000,
            }],
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no puedes recibir"), "{err}");
    }
}
