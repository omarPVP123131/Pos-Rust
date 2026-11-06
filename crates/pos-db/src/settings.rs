//! Configuración de la sucursal (nombre del negocio, impresora, ancho de
//! ticket, etc.). Viaja en la base de datos para que el respaldo la incluya.

use crate::Result;
use rusqlite::{params, Connection};

pub struct Settings;

impl Settings {
    pub fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
        let valor = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(valor)
    }

    pub fn get_or(conn: &Connection, key: &str, defecto: &str) -> String {
        Self::get(conn, key)
            .ok()
            .flatten()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| defecto.to_string())
    }

    pub fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                            updated_at = datetime('now')",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn as_i64(conn: &Connection, key: &str, defecto: i64) -> i64 {
        Self::get(conn, key)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(defecto)
    }
}

// `optional()` viene de `rusqlite::OptionalExtension`.
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[test]
    fn lee_escribe_y_actualiza() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            Settings::get_or(db.conn(), "store_name", "Sin nombre"),
            "Mi Tienda"
        );
        Settings::set(db.conn(), "store_name", "Abarrotes Lupita").unwrap();
        assert_eq!(
            Settings::get(db.conn(), "store_name").unwrap().unwrap(),
            "Abarrotes Lupita"
        );
        assert_eq!(Settings::as_i64(db.conn(), "ticket_width", 32), 48);
        Settings::set(db.conn(), "inexistente", "1").unwrap();
        assert_eq!(Settings::as_i64(db.conn(), "inexistente", 7), 1);
        // Un valor no numérico cae al defecto sin romper nada.
        Settings::set(db.conn(), "ticket_width", "ancho").unwrap();
        assert_eq!(Settings::as_i64(db.conn(), "ticket_width", 32), 32);
    }
}
