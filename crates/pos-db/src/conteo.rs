//! Conteo físico asistido.
//!
//! El sistema dice cuánto *debería* haber; alguien con una libreta (o con el
//! escáner) cuenta cuánto hay de verdad. La diferencia es la merma, y es el
//! número que una tienda casi nunca tiene.
//!
//! Dos reglas de diseño:
//!
//! 1. **Contar lleva horas y la tienda sigue vendiendo.** Por eso el conteo se
//!    guarda renglón por renglón: si se cierra el programa a media cuenta, lo
//!    contado sigue ahí.
//! 2. **La diferencia se calcula contra el stock de ahora**, no contra la foto
//!    del principio. Si contaste 10 latas a las 10 de la mañana y a las 11 se
//!    vendieron 2, el ajuste debe ser sobre el stock actual, porque la venta
//!    ya movió el inventario. La foto inicial (`esperado`) se conserva sólo
//!    para que la persona vea cómo arrancó.

use rusqlite::{params, Connection};

use crate::products::adjust_stock_tx;
use crate::{DbError, Result};

/// Un conteo en curso (o ya cerrado).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conteo {
    pub id: i64,
    pub creado_por: Option<i64>,
    pub alcance: String,
    pub filtro: String,
    /// `abierto` | `cerrado` | `aplicado`
    pub estado: String,
    pub created_at: String,
}

/// Un producto dentro del conteo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenglonConteo {
    pub product_id: i64,
    pub sku: String,
    pub nombre: String,
    pub unidad: String,
    pub categoria: Option<String>,
    /// Lo que el sistema decía al empezar (foto).
    pub esperado_millis: i64,
    /// Lo que contó la persona; `None` si aún no se cuenta.
    pub contado_millis: Option<i64>,
    /// El stock en este momento (puede haberse movido por ventas).
    pub actual_millis: i64,
    pub costo_cents: i64,
}

impl RenglonConteo {
    /// Diferencia contra el stock actual: lo que hay que ajustar.
    pub fn diferencia_millis(&self) -> Option<i64> {
        self.contado_millis.map(|c| c - self.actual_millis)
    }
}

/// Resumen de un conteo, para la pantalla y para el reporte.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResumenConteo {
    pub productos: usize,
    pub contados: usize,
    pub con_diferencia: usize,
    /// Unidades que faltan (merma), en milésimas.
    pub faltantes_millis: i64,
    /// Unidades que sobran, en milésimas.
    pub sobrantes_millis: i64,
    /// Valor aproximado de la merma, a costo.
    pub valor_merma_cents: i64,
}

impl ResumenConteo {
    fn vacio() -> ResumenConteo {
        ResumenConteo::default()
    }
}

/// Arranca un conteo: toma la foto de todos los productos con inventario
/// (opcionalmente los de una sola categoría).
pub fn iniciar(conn: &mut Connection, user_id: i64, filtro_categoria: &str) -> Result<i64> {
    // Sólo puede haber un conteo abierto: dos conteos a la vez serían un
    // desastre de diferencias cruzadas.
    if abierto(conn)?.is_some() {
        return Err(DbError::Negocio(
            "Ya hay un conteo abierto. Ciérralo o cancélalo antes de empezar otro.".into(),
        ));
    }

    let tx = conn.transaction()?;
    let (alcance, filtro) = if filtro_categoria.trim().is_empty() {
        ("todos", String::new())
    } else {
        ("categoria", filtro_categoria.trim().to_string())
    };
    tx.execute(
        "INSERT INTO stock_counts (creado_por, alcance, filtro, estado)
         VALUES (?1, ?2, ?3, 'abierto')",
        params![user_id, alcance, filtro],
    )?;
    let id: i64 = tx.last_insert_rowid();

    let productos: Vec<(i64, i64)> = {
        let mut filas: Vec<(i64, i64)> = Vec::new();
        if alcance == "todos" {
            let mut stmt = tx.prepare(
                "SELECT id, stock_millis FROM products
                 WHERE track_inventory = 1 AND is_active = 1
                 ORDER BY name COLLATE NOCASE",
            )?;
            let mut q = stmt.query([])?;
            while let Some(r) = q.next()? {
                filas.push((r.get(0)?, r.get(1)?));
            }
        } else {
            let mut stmt = tx.prepare(
                "SELECT p.id, p.stock_millis FROM products p
                 JOIN categories c ON c.id = p.category_id
                 WHERE p.track_inventory = 1 AND p.is_active = 1
                   AND lower(c.name) = lower(?1)
                 ORDER BY p.name COLLATE NOCASE",
            )?;
            let mut q = stmt.query(params![filtro])?;
            while let Some(r) = q.next()? {
                filas.push((r.get(0)?, r.get(1)?));
            }
        }
        filas
    };

    if productos.is_empty() {
        return Err(DbError::Negocio(
            "No hay productos que contar con ese filtro".into(),
        ));
    }

    for (producto, stock) in &productos {
        tx.execute(
            "INSERT INTO stock_count_lines (count_id, product_id, esperado_millis)
             VALUES (?1, ?2, ?3)",
            params![id, producto, stock],
        )?;
    }
    tx.commit()?;
    Ok(id)
}

/// El conteo abierto, si hay uno.
pub fn abierto(conn: &Connection) -> Result<Option<Conteo>> {
    conn.query_row(
        "SELECT id, creado_por, alcance, filtro, estado,
                datetime(created_at, '-6 hours')
         FROM stock_counts WHERE estado = 'abierto'
         ORDER BY id DESC LIMIT 1",
        [],
        |r| {
            Ok(Conteo {
                id: r.get(0)?,
                creado_por: r.get(1)?,
                alcance: r.get(2)?,
                filtro: r.get(3)?,
                estado: r.get(4)?,
                created_at: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

use rusqlite::OptionalExtension;

pub fn por_id(conn: &Connection, id: i64) -> Result<Option<Conteo>> {
    conn.query_row(
        "SELECT id, creado_por, alcance, filtro, estado,
                datetime(created_at, '-6 hours')
         FROM stock_counts WHERE id = ?1",
        params![id],
        |r| {
            Ok(Conteo {
                id: r.get(0)?,
                creado_por: r.get(1)?,
                alcance: r.get(2)?,
                filtro: r.get(3)?,
                estado: r.get(4)?,
                created_at: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Los renglones del conteo, con el stock actual al lado.
pub fn renglones(conn: &Connection, conteo_id: i64) -> Result<Vec<RenglonConteo>> {
    let mut stmt = conn.prepare(
        "SELECT l.product_id, p.sku, p.name, p.unit, c.name,
                l.esperado_millis, l.contado_millis, p.stock_millis, p.cost_cents
         FROM stock_count_lines l
         JOIN products p ON p.id = l.product_id
         LEFT JOIN categories c ON c.id = p.category_id
         WHERE l.count_id = ?1
         ORDER BY p.name COLLATE NOCASE",
    )?;
    let filas = stmt
        .query_map(params![conteo_id], |r| {
            Ok(RenglonConteo {
                product_id: r.get(0)?,
                sku: r.get(1)?,
                nombre: r.get(2)?,
                unidad: r.get(3)?,
                categoria: r.get(4)?,
                esperado_millis: r.get(5)?,
                contado_millis: r.get(6)?,
                actual_millis: r.get(7)?,
                costo_cents: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(filas)
}

/// Anota lo que se contó de un producto. Se puede corregir: volver a contar
/// lo mismo sobrescribe el número anterior.
pub fn contar(
    conn: &Connection,
    conteo_id: i64,
    product_id: i64,
    cantidad_millis: i64,
) -> Result<()> {
    if cantidad_millis < 0 {
        return Err(DbError::Negocio(
            "No se puede contar una cantidad negativa".into(),
        ));
    }
    let existe: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM stock_count_lines WHERE count_id = ?1 AND product_id = ?2)",
        params![conteo_id, product_id],
        |r| r.get(0),
    )?;
    if !existe {
        return Err(DbError::Negocio("Ese producto no está en el conteo".into()));
    }
    conn.execute(
        "UPDATE stock_count_lines
         SET contado_millis = ?1, contado_at = datetime('now')
         WHERE count_id = ?2 AND product_id = ?3",
        params![cantidad_millis, conteo_id, product_id],
    )?;
    Ok(())
}

/// Quita lo contado de un producto (para volverlo a contar después).
pub fn descontar(conn: &Connection, conteo_id: i64, product_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE stock_count_lines
         SET contado_millis = NULL, contado_at = NULL
         WHERE count_id = ?1 AND product_id = ?2",
        params![conteo_id, product_id],
    )?;
    Ok(())
}

/// El resumen de cómo va (o cómo quedó) el conteo.
pub fn resumen(conn: &Connection, conteo_id: i64) -> Result<ResumenConteo> {
    let filas = renglones(conn, conteo_id)?;
    let mut r = ResumenConteo::vacio();
    r.productos = filas.len();
    for f in &filas {
        let Some(contado) = f.contado_millis else {
            continue;
        };
        r.contados += 1;
        let diferencia = contado - f.actual_millis;
        if diferencia == 0 {
            continue;
        }
        r.con_diferencia += 1;
        if diferencia < 0 {
            r.faltantes_millis += -diferencia;
        } else {
            r.sobrantes_millis += diferencia;
        }
        // La merma se valúa a costo: es lo que realmente se perdió.
        let unidades = (diferencia.abs() as f64 / 1000.0).ceil() as i64;
        r.valor_merma_cents += if diferencia < 0 {
            unidades * f.costo_cents
        } else {
            0
        };
    }
    Ok(r)
}

/// Cierra el conteo. Si `aplicar` es verdadero, escribe los ajustes de
/// inventario (con su motivo) y deja el conteo como `aplicado`.
///
/// Sólo toca los productos que **sí se contaron**: lo que nadie contó queda
/// como está, en vez de ajustarse a cero.
pub fn cerrar(
    conn: &mut Connection,
    conteo_id: i64,
    user_id: i64,
    aplicar: bool,
) -> Result<ResumenConteo> {
    let conteo = por_id(conn, conteo_id)?
        .ok_or_else(|| DbError::Negocio(format!("No existe el conteo {conteo_id}")))?;
    if conteo.estado != "abierto" {
        return Err(DbError::Negocio("Ese conteo ya está cerrado".into()));
    }

    let filas = renglones(conn, conteo_id)?;
    let resumen = resumen(conn, conteo_id)?;

    if aplicar {
        let tx = conn.transaction()?;
        let motivo = format!("conteo físico #{conteo_id}");
        for f in &filas {
            let Some(contado) = f.contado_millis else {
                continue;
            };
            let delta = contado - f.actual_millis;
            if delta != 0 {
                adjust_stock_tx(&tx, f.product_id, delta, &motivo, user_id, None, None)?;
            }
        }
        tx.execute(
            "UPDATE stock_counts
             SET estado = 'aplicado', cerrado_at = datetime('now'), aplicado_por = ?1
             WHERE id = ?2",
            params![user_id, conteo_id],
        )?;
        tx.commit()?;
    } else {
        conn.execute(
            "UPDATE stock_counts
             SET estado = 'cerrado', cerrado_at = datetime('now'), aplicado_por = ?1
             WHERE id = ?2",
            params![user_id, conteo_id],
        )?;
    }
    Ok(resumen)
}

/// Cancela un conteo abierto: tira lo contado, no toca el inventario.
pub fn cancelar(conn: &Connection, conteo_id: i64) -> Result<()> {
    let conteo = por_id(conn, conteo_id)?
        .ok_or_else(|| DbError::Negocio(format!("No existe el conteo {conteo_id}")))?;
    if conteo.estado != "abierto" {
        return Err(DbError::Negocio(
            "Un conteo cerrado o aplicado no se puede cancelar".into(),
        ));
    }
    conn.execute(
        "DELETE FROM stock_count_lines WHERE count_id = ?1",
        params![conteo_id],
    )?;
    conn.execute("DELETE FROM stock_counts WHERE id = ?1", params![conteo_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::products::{NewProduct, Products};
    use crate::Database;

    fn base() -> (Database, i64, i64) {
        let mut db = Database::open_in_memory().unwrap();
        let a = Products::insert(
            db.conn_mut(),
            &NewProduct {
                sku: "7501".into(),
                name: "Coca-Cola 600 ml".into(),
                price_cents: 1_800,
                cost_cents: 1_200,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: 100_000,
                min_stock_millis: 5_000,
                track_inventory: true,
                barcodes: vec![],
            },
        )
        .unwrap();
        let b = Products::insert(
            db.conn_mut(),
            &NewProduct {
                sku: "7502".into(),
                name: "Pan blanco".into(),
                price_cents: 1_850,
                cost_cents: 1_300,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: 30_000,
                min_stock_millis: 5_000,
                track_inventory: true,
                barcodes: vec![],
            },
        )
        .unwrap();
        (db, a, b)
    }

    #[test]
    fn iniciar_toma_la_foto_del_inventario() {
        let (mut db, a, _b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        let filas = renglones(db.conn(), id).unwrap();
        assert_eq!(filas.len(), 2);
        let coca = filas.iter().find(|f| f.product_id == a).unwrap();
        assert_eq!(coca.esperado_millis, 100_000);
        assert_eq!(coca.actual_millis, 100_000);
        assert_eq!(coca.contado_millis, None, "arranca sin contar nada");
        assert_eq!(coca.diferencia_millis(), None);
    }

    #[test]
    fn no_puede_haber_dos_conteos_abiertos() {
        let (mut db, _, _) = base();
        iniciar(db.conn_mut(), 1, "").unwrap();
        let e = iniciar(db.conn_mut(), 1, "").unwrap_err();
        assert!(e.to_string().contains("conteo abierto"), "{e}");
    }

    #[test]
    fn contar_y_recontar_un_producto() {
        let (mut db, a, _b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        contar(db.conn(), id, a, 98_500).unwrap();

        let filas = renglones(db.conn(), id).unwrap();
        let coca = filas.iter().find(|f| f.product_id == a).unwrap();
        assert_eq!(coca.contado_millis, Some(98_500));
        // La diferencia es contra el stock de ahora, no contra la foto.
        assert_eq!(coca.diferencia_millis(), Some(-1_500));

        // Volver a contar corrige, no suma.
        contar(db.conn(), id, a, 100_000).unwrap();
        let filas = renglones(db.conn(), id).unwrap();
        let coca = filas.iter().find(|f| f.product_id == a).unwrap();
        assert_eq!(coca.contado_millis, Some(100_000));
        assert_eq!(coca.diferencia_millis(), Some(0));

        // Y se puede dejar sin contar otra vez.
        descontar(db.conn(), id, a).unwrap();
        let filas = renglones(db.conn(), id).unwrap();
        assert!(filas
            .iter()
            .find(|f| f.product_id == a)
            .unwrap()
            .contado_millis
            .is_none());
    }

    #[test]
    fn contar_un_producto_que_no_esta_en_el_conteo_falla() {
        let (mut db, _, _) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        let e = contar(db.conn(), id, 9_999, 1000).unwrap_err();
        assert!(e.to_string().contains("no está en el conteo"), "{e}");
    }

    #[test]
    fn la_diferencia_se_calcula_contra_el_stock_de_ahora() {
        let (mut db, a, _b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        // Mientras se cuenta, se venden 10 refrescos.
        Products::adjust_stock(db.conn_mut(), a, -10_000, "venta", 1).unwrap();

        // La persona cuenta 95: contra la foto parecen 5 menos, pero contra el
        // stock real (90) son 5 más. Vale lo que dice el anaquel, no la foto.
        contar(db.conn(), id, a, 95_000).unwrap();
        let filas = renglones(db.conn(), id).unwrap();
        let coca = filas.iter().find(|f| f.product_id == a).unwrap();
        assert_eq!(coca.actual_millis, 90_000);
        assert_eq!(coca.diferencia_millis(), Some(5_000));
    }

    #[test]
    fn cerrar_sin_aplicar_no_toca_el_inventario() {
        let (mut db, a, _b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        contar(db.conn(), id, a, 90_000).unwrap();
        let r = cerrar(db.conn_mut(), id, 1, false).unwrap();
        assert_eq!(r.contados, 1);
        assert_eq!(r.con_diferencia, 1);
        assert_eq!(r.faltantes_millis, 10_000);

        let producto = Products::get(db.conn(), a).unwrap().unwrap();
        assert_eq!(
            producto.stock_millis, 100_000,
            "sin aplicar, el inventario no se mueve"
        );
        assert_eq!(por_id(db.conn(), id).unwrap().unwrap().estado, "cerrado");
    }

    #[test]
    fn aplicar_el_conteo_ajusta_el_inventario_y_cuadra() {
        let (mut db, a, b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        contar(db.conn(), id, a, 90_000).unwrap(); // faltan 10
        contar(db.conn(), id, b, 31_000).unwrap(); // sobran 1
        let r = cerrar(db.conn_mut(), id, 1, true).unwrap();

        assert_eq!(r.con_diferencia, 2);
        assert_eq!(r.faltantes_millis, 10_000);
        assert_eq!(r.sobrantes_millis, 1_000);
        // 10 x $12.00 de costo
        assert_eq!(r.valor_merma_cents, 120_000 / 10);

        assert_eq!(
            Products::get(db.conn(), a).unwrap().unwrap().stock_millis,
            90_000
        );
        assert_eq!(
            Products::get(db.conn(), b).unwrap().unwrap().stock_millis,
            31_000
        );

        // Y el inventario sigue cuadrando con su libro de movimientos.
        assert!(
            db.conn()
                .query_row(
                    "SELECT NOT EXISTS(
                        SELECT 1 FROM products p
                        WHERE p.track_inventory = 1
                          AND p.stock_millis <> COALESCE((SELECT SUM(qty_delta_millis)
                              FROM inventory_movements m WHERE m.product_id = p.id), 0))",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap(),
            "el ajuste del conteo deja el inventario cuadrado"
        );
    }

    #[test]
    fn lo_que_nadie_conto_no_se_ajusta_a_cero() {
        let (mut db, a, b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        // Sólo se contó la Coca; el Pan nadie lo tocó.
        contar(db.conn(), id, a, 90_000).unwrap();
        cerrar(db.conn_mut(), id, 1, true).unwrap();
        assert_eq!(
            Products::get(db.conn(), b).unwrap().unwrap().stock_millis,
            30_000,
            "no contar algo no es lo mismo que contar cero"
        );
    }

    #[test]
    fn cancelar_tira_el_conteo_y_libera_la_caja() {
        let (mut db, a, _b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        contar(db.conn(), id, a, 5_000).unwrap();
        cancelar(db.conn(), id).unwrap();
        assert!(abierto(db.conn()).unwrap().is_none());
        assert!(por_id(db.conn(), id).unwrap().is_none());
        // Y ya se puede empezar otro.
        assert!(iniciar(db.conn_mut(), 1, "").is_ok());
    }

    #[test]
    fn un_conteo_aplicado_no_se_puede_cerrar_dos_veces() {
        let (mut db, a, _b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        contar(db.conn(), id, a, 99_000).unwrap();
        cerrar(db.conn_mut(), id, 1, true).unwrap();
        let e = cerrar(db.conn_mut(), id, 1, true).unwrap_err();
        assert!(e.to_string().contains("cerrado"), "{e}");
    }

    #[test]
    fn el_resumen_cuenta_bien() {
        let (mut db, a, b) = base();
        let id = iniciar(db.conn_mut(), 1, "").unwrap();
        assert_eq!(resumen(db.conn(), id).unwrap().contados, 0);

        contar(db.conn(), id, a, 100_000).unwrap(); // sin diferencia
        let r = resumen(db.conn(), id).unwrap();
        assert_eq!(r.productos, 2);
        assert_eq!(r.contados, 1);
        assert_eq!(r.con_diferencia, 0);

        contar(db.conn(), id, b, 28_000).unwrap(); // faltan 2
        let r = resumen(db.conn(), id).unwrap();
        assert_eq!(r.contados, 2);
        assert_eq!(r.con_diferencia, 1);
        assert_eq!(r.faltantes_millis, 2_000);
        assert_eq!(r.valor_merma_cents, 2 * 1_300);
    }
}
