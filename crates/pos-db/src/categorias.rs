//! Categorías del catálogo.
//!
//! Son el primer filtro cuando el catálogo crece: con 200 productos nadie
//! encuentra nada sin agrupar. El esquema ya las tenía desde la migración 0001;
//! este módulo las expone.

use rusqlite::{params, Connection};

use crate::{DbError, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Categoria {
    pub id: i64,
    pub name: String,
}

pub struct Categories;

impl Categories {
    pub fn list(conn: &Connection) -> Result<Vec<Categoria>> {
        let mut stmt = conn.prepare("SELECT id, name FROM categories ORDER BY name")?;
        let filas = stmt
            .query_map([], |r| {
                Ok(Categoria {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    /// Crea una categoría si no existe y devuelve su id (útil al importar CSV).
    pub fn asegurar(conn: &Connection, nombre: &str) -> Result<i64> {
        let nombre = nombre.trim();
        if nombre.is_empty() {
            return Err(DbError::Negocio("La categoría necesita un nombre".into()));
        }
        let existente: Option<i64> = conn
            .query_row(
                "SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE",
                params![nombre],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existente {
            return Ok(id);
        }
        conn.execute("INSERT INTO categories (name) VALUES (?1)", params![nombre])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn renombrar(conn: &Connection, id: i64, nombre: &str) -> Result<()> {
        let nombre = nombre.trim();
        if nombre.is_empty() {
            return Err(DbError::Negocio("La categoría necesita un nombre".into()));
        }
        conn.execute(
            "UPDATE categories SET name = ?1 WHERE id = ?2",
            params![nombre, id],
        )?;
        Ok(())
    }

    /// Cuántos productos activos hay en cada categoría.
    pub fn con_conteo(conn: &Connection) -> Result<Vec<(Categoria, i64)>> {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.name, COUNT(p.id)
             FROM categories c
             LEFT JOIN products p ON p.category_id = c.id AND p.is_active = 1
             GROUP BY c.id
             ORDER BY c.name",
        )?;
        let filas = stmt
            .query_map([], |r| {
                Ok((
                    Categoria {
                        id: r.get(0)?,
                        name: r.get(1)?,
                    },
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }
}

/// Pon (o quita, con `None`) la categoría de un producto.
pub fn asignar(conn: &Connection, product_id: i64, category_id: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE products SET category_id = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![category_id, product_id],
    )?;
    Ok(())
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::products::{NewProduct, Products};
    use crate::Database;

    fn producto(db: &mut Database, sku: &str) -> i64 {
        Products::insert(
            db.conn_mut(),
            &NewProduct {
                sku: sku.into(),
                name: format!("Producto {sku}"),
                price_cents: 1_000,
                cost_cents: 500,
                tax_bp: 1_600,
                unit: "pza".into(),
                stock_millis: 0,
                min_stock_millis: 0,
                track_inventory: true,
                barcodes: vec![],
            },
        )
        .unwrap()
    }

    #[test]
    fn asegurar_no_duplica_la_categoria() {
        let db = Database::open_in_memory().unwrap();
        let a = Categories::asegurar(db.conn(), "Abarrotes").unwrap();
        let b = Categories::asegurar(db.conn(), "abarrotes").unwrap();
        assert_eq!(a, b, "debe reconocerla sin importar mayúsculas");
        assert_eq!(Categories::list(db.conn()).unwrap().len(), 1);
    }

    #[test]
    fn una_categoria_vacia_no_pasa() {
        let db = Database::open_in_memory().unwrap();
        assert!(Categories::asegurar(db.conn(), "   ").is_err());
    }

    #[test]
    fn asignar_y_contar_productos() {
        let mut db = Database::open_in_memory().unwrap();
        let abarrotes = Categories::asegurar(db.conn(), "Abarrotes").unwrap();
        let id1 = producto(&mut db, "A1");
        let id2 = producto(&mut db, "A2");
        asignar(db.conn(), id1, Some(abarrotes)).unwrap();
        asignar(db.conn(), id2, Some(abarrotes)).unwrap();

        let conteos = Categories::con_conteo(db.conn()).unwrap();
        assert_eq!(conteos.len(), 1);
        assert_eq!(conteos[0].1, 2);

        // Quitar la categoría la deja en blanco.
        asignar(db.conn(), id2, None).unwrap();
        assert_eq!(Categories::con_conteo(db.conn()).unwrap()[0].1, 1);
    }
}
