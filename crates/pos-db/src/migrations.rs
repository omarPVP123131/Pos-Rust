//! Migraciones versionadas con `PRAGMA user_version`.
//!
//! Cada archivo en `migrations/` es una migración; se aplican en orden dentro
//! de una transacción. Así un POS instalado hace un año se actualiza solo.

use rusqlite::Connection;

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_init",
        include_str!("../../../migrations/0001_init.sql"),
    ),
    (
        "0002_usuarios",
        include_str!("../../../migrations/0002_usuarios.sql"),
    ),
    (
        "0003_cartera",
        include_str!("../../../migrations/0003_cartera.sql"),
    ),
    (
        "0004_conteo",
        include_str!("../../../migrations/0004_conteo.sql"),
    ),
    (
        "0005_fiscal",
        include_str!("../../../migrations/0005_fiscal.sql"),
    ),
    (
        "0006_notas_y_claves",
        include_str!("../../../migrations/0006_notas_y_claves.sql"),
    ),
    (
        "0007_factura_individual",
        include_str!("../../../migrations/0007_factura_individual.sql"),
    ),
    (
        "0008_complemento_pagos",
        include_str!("../../../migrations/0008_complemento_pagos.sql"),
    ),
    (
        "0009_cancelacion",
        include_str!("../../../migrations/0009_cancelacion.sql"),
    ),
    (
        "0010_pedidos",
        include_str!("../../../migrations/0010_pedidos.sql"),
    ),
];

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version_actual: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    for (i, (nombre, sql)) in MIGRATIONS.iter().enumerate() {
        let objetivo = (i + 1) as i64;
        if version_actual >= objetivo {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", objetivo)?;
        tx.commit()?;
        let _ = nombre; // nombre del archivo, útil para bitácora de arranque
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aplica_las_migraciones_una_sola_vez() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let v1: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v1, MIGRATIONS.len() as i64);

        // Segunda llamada: idempotente.
        migrate(&conn).unwrap();
        let v2: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v1, v2);

        // Y el esquema sembrado quedó listo.
        let tasas: i64 = conn
            .query_row("SELECT COUNT(*) FROM tax_rates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tasas, 4);
    }
}
