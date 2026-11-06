//! Bitácora de auditoría: quién hizo qué, y quién autorizó qué.
//!
//! Cuando falta dinero en el cajón, la pregunta no es "¿cuánto falta?" sino
//! "**¿quién y cuándo?**". Esta tabla es la respuesta: cada venta, cada
//! devolución, cada corte, cada cambio de precio autorizado y cada respaldo
//! queda anotado con su usuario y su hora de la tienda.
//!
//! Dos reglas:
//!
//! 1. **Nunca se borra.** No hay comando para limpiar la bitácora a propósito.
//! 2. **Fallar no puede tumbar una venta.** [`registrar`] se usa para anotar;
//!    si algo sale mal, quien llama decide ignorarlo (así se hace en el API).

use rusqlite::{params, Connection};

use crate::sales::dia_local;
use crate::{DbError, Result};

/// Un renglón de la bitácora, ya con el nombre del usuario resuelto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evento {
    pub id: i64,
    /// Nombre para mostrar; "—" cuando el evento es del sistema (un respaldo).
    pub usuario: String,
    pub accion: String,
    pub entidad: String,
    pub entidad_id: Option<i64>,
    pub detalle: String,
    /// Hora **de la tienda** (`YYYY-MM-DD HH:MM:SS`).
    pub created_at: String,
}

/// Criterios de búsqueda de la pantalla de auditoría.
#[derive(Debug, Clone, Default)]
pub struct Filtro {
    /// `YYYY-MM-DD`; vacío = sin límite inferior.
    pub desde: String,
    pub hasta: String,
    /// Nombre o usuario (busca por parecido).
    pub usuario: String,
    /// Acción exacta (`venta`, `autorizar`...); vacío = todas.
    pub accion: String,
    /// Texto libre dentro del detalle.
    pub texto: String,
    pub limite: usize,
}

/// Etiqueta en español de cada acción conocida.
///
/// Las que no conoce las devuelve igual, para que un evento nuevo no se
/// pierda por no estar en esta lista.
pub fn etiqueta(accion: &str) -> String {
    String::from(match accion {
        "venta" => "Venta",
        "cancelacion" => "Devolución",
        "alta" => "Producto nuevo",
        "edicion" => "Producto editado",
        "baja" => "Producto dado de baja",
        "ajuste_inventario" => "Ajuste de inventario",
        "importacion" => "Importación de catálogo",
        "abrir_caja" => "Abrió caja",
        "cerrar_caja" => "Cortó caja",
        "movimiento_caja" => "Movimiento de caja",
        "login" => "Entró al sistema",
        "login_fallido" => "PIN equivocado",
        "bloqueo" => "Usuario bloqueado",
        "abono" => "Abono a cartera",
        "autorizar" => "Autorización",
        "precio" => "Precio a mano",
        "descuento" => "Descuento",
        "fiado" => "Fiado",
        "diferencia" => "Corte que no cuadra",
        "retiro" => "Retiro o gasto",
        "pac_http" => "PAC por HTTP bloqueado",
        "impresion_prueba" => "Ticket de prueba",
        "impresion_prueba_fallida" => "Ticket de prueba fallido",
        "respaldo" => "Respaldo",
        "configuracion" => "Cambió la configuración",
        "iniciar_conteo" => "Empezó un conteo físico",
        "cerrar_conteo" => "Cerró un conteo físico",
        conocida => conocida,
    })
}

/// Anota un evento. `detalle` es texto libre para humanos.
pub fn registrar(
    conn: &Connection,
    usuario_id: Option<i64>,
    accion: &str,
    entidad: &str,
    entidad_id: Option<i64>,
    detalle: &str,
) -> Result<()> {
    if accion.trim().is_empty() || entidad.trim().is_empty() {
        return Err(DbError::Negocio(
            "Un evento de auditoría necesita acción y entidad".into(),
        ));
    }
    conn.execute(
        "INSERT INTO audit_log (user_id, action, entity, entity_id, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![usuario_id, accion, entidad, entidad_id, detalle],
    )?;
    Ok(())
}

/// Igual que [`registrar`] pero **nunca falla**: para no tumbar una venta por
/// no poder escribir la bitácora.
pub fn anotar(
    conn: &Connection,
    usuario_id: Option<i64>,
    accion: &str,
    entidad: &str,
    entidad_id: Option<i64>,
    detalle: &str,
) {
    if let Err(_e) = registrar(conn, usuario_id, accion, entidad, entidad_id, detalle) {
        // No imprimir: en Windows sin consola, eprintln! aborta el proceso
        // (justo al entrar, que es cuando se anota el login).
    }
}

/// Busca eventos. Los más recientes primero.
pub fn buscar(conn: &Connection, filtro: &Filtro) -> Result<Vec<Evento>> {
    let dia = dia_local("a.created_at");
    let mut sql = String::from(
        "SELECT a.id,
                COALESCE(u.display_name, u.username, '—'),
                a.action, a.entity, a.entity_id,
                COALESCE(a.payload_json, ''),
                datetime(a.created_at, '-6 hours')
         FROM audit_log a
         LEFT JOIN users u ON u.id = a.user_id
         WHERE 1 = 1",
    );
    let mut valores: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !filtro.desde.trim().is_empty() {
        sql.push_str(&format!(" AND {dia} >= date(?{})", valores.len() + 1));
        valores.push(Box::new(filtro.desde.trim().to_string()));
    }
    if !filtro.hasta.trim().is_empty() {
        sql.push_str(&format!(" AND {dia} <= date(?{})", valores.len() + 1));
        valores.push(Box::new(filtro.hasta.trim().to_string()));
    }
    if !filtro.accion.trim().is_empty() {
        sql.push_str(&format!(" AND a.action = ?{}", valores.len() + 1));
        valores.push(Box::new(filtro.accion.trim().to_string()));
    }
    if !filtro.usuario.trim().is_empty() {
        sql.push_str(&format!(
            " AND COALESCE(u.display_name, u.username, '') LIKE ?{}",
            valores.len() + 1
        ));
        valores.push(Box::new(format!("%{}%", filtro.usuario.trim())));
    }
    if !filtro.texto.trim().is_empty() {
        sql.push_str(&format!(
            " AND (COALESCE(a.payload_json, '') LIKE ?{}
                   OR COALESCE(u.display_name, u.username, '') LIKE ?{}
                   OR a.action LIKE ?{})",
            valores.len() + 1,
            valores.len() + 2,
            valores.len() + 3
        ));
        let como = format!("%{}%", filtro.texto.trim());
        valores.push(Box::new(como.clone()));
        valores.push(Box::new(como.clone()));
        valores.push(Box::new(como));
    }

    sql.push_str(&format!(" ORDER BY a.id DESC LIMIT ?{}", valores.len() + 1));
    let limite = if filtro.limite == 0 {
        200
    } else {
        filtro.limite
    };
    valores.push(Box::new(limite as i64));

    let mut stmt = conn.prepare(&sql)?;
    let filas = stmt
        .query_map(
            rusqlite::params_from_iter(valores.iter().map(|v| v.as_ref())),
            |r| {
                Ok(Evento {
                    id: r.get(0)?,
                    usuario: r.get(1)?,
                    accion: r.get(2)?,
                    entidad: r.get(3)?,
                    entidad_id: r.get(4)?,
                    detalle: r.get(5)?,
                    created_at: r.get(6)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(filas)
}

/// Las acciones que de verdad hay en la bitácora: para llenar el filtro de la
/// pantalla sin adivinar.
pub fn acciones(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT action FROM audit_log ORDER BY action")?;
    let filas = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(filas)
}

/// Cuántos eventos hay en total (para saber si el filtro está escondiendo algo).
pub fn total(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))?)
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_acciones_del_conteo_tienen_etiqueta_en_espanol() {
        assert_eq!(etiqueta("iniciar_conteo"), "Empezó un conteo físico");
        assert_eq!(etiqueta("cerrar_conteo"), "Cerró un conteo físico");
        // Una acción nueva no se pierde: se muestra tal cual.
        assert_eq!(etiqueta("algo_nuevo"), "algo_nuevo");
    }
}

#[cfg(test)]
mod tests_anteriores {
    use super::*;
    use crate::users::Users;
    use crate::Database;
    use pos_core::Role;

    fn base() -> (Database, i64) {
        let mut db = Database::open_in_memory().unwrap();
        let id = Users::crear(db.conn_mut(), "ana", "Ana Ruiz", "4321", Role::Cajero).unwrap();
        (db, id)
    }

    fn anotar_usuario(db: &Database, id: i64, accion: &str, detalle: &str) {
        registrar(db.conn(), Some(id), accion, "sale", Some(7), detalle).unwrap();
    }

    #[test]
    fn se_registra_y_se_lee_con_el_nombre_del_usuario() {
        let (db, id) = base();
        anotar_usuario(&db, id, "venta", "Folio 000001 · $36.00");

        let todos = buscar(db.conn(), &Filtro::default()).unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].usuario, "Ana Ruiz");
        assert_eq!(todos[0].accion, "venta");
        assert_eq!(todos[0].detalle, "Folio 000001 · $36.00");
        assert_eq!(todos[0].entidad_id, Some(7));
        assert!(!todos[0].created_at.is_empty());
    }

    #[test]
    fn un_evento_del_sistema_sale_con_guion() {
        let (db, _) = base();
        registrar(
            db.conn(),
            None,
            "respaldo",
            "system",
            None,
            "respaldo-20261010.db",
        )
        .unwrap();
        let todos = buscar(db.conn(), &Filtro::default()).unwrap();
        assert_eq!(todos[0].usuario, "—");
    }

    #[test]
    fn los_mas_recientes_primero() {
        let (db, id) = base();
        for i in 0..5 {
            anotar_usuario(&db, id, "venta", &format!("venta {i}"));
        }
        let todos = buscar(db.conn(), &Filtro::default()).unwrap();
        assert_eq!(todos[0].detalle, "venta 4");
        assert!(todos[0].id > todos[4].id);
    }

    #[test]
    fn el_filtro_por_accion_y_por_texto_funciona() {
        let (db, id) = base();
        anotar_usuario(&db, id, "venta", "Folio 000001");
        anotar_usuario(&db, id, "autorizar", "Ana cambió el precio");
        anotar_usuario(&db, id, "cerrar_caja", "Corte del día");

        let f = Filtro {
            accion: "autorizar".into(),
            ..Default::default()
        };
        let r = buscar(db.conn(), &f).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].detalle, "Ana cambió el precio");

        let f = Filtro {
            texto: "000001".into(),
            ..Default::default()
        };
        assert_eq!(buscar(db.conn(), &f).unwrap().len(), 1);

        let f = Filtro {
            texto: "Ana".into(),
            ..Default::default()
        };
        // "Ana" está en el nombre del usuario de los tres y en un detalle.
        assert_eq!(buscar(db.conn(), &f).unwrap().len(), 3);
    }

    #[test]
    fn el_filtro_por_usuario_busca_por_parecido() {
        let (mut db, ana) = base();
        let jefe =
            Users::crear(db.conn_mut(), "jefe", "El Jefe", "2222", Role::Supervisor).unwrap();
        anotar_usuario(&db, ana, "venta", "una");
        anotar_usuario(&db, jefe, "autorizar", "otra");

        let f = Filtro {
            usuario: "Jefe".into(),
            ..Default::default()
        };
        let r = buscar(db.conn(), &f).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].usuario, "El Jefe");
    }

    #[test]
    fn el_filtro_por_fecha_usa_la_hora_de_la_tienda() {
        let (db, id) = base();
        // 2026-10-11 02:00 UTC = 2026-10-10 20:00 en México: es del 10.
        db.conn()
            .execute(
                "INSERT INTO audit_log (user_id, action, entity, payload_json, created_at)
                 VALUES (?1, 'venta', 'sale', 'de noche', '2026-10-11 02:00:00')",
                params![id],
            )
            .unwrap();

        let f = Filtro {
            desde: "2026-10-10".into(),
            hasta: "2026-10-10".into(),
            ..Default::default()
        };
        assert_eq!(
            buscar(db.conn(), &f).unwrap().len(),
            1,
            "el filtro de fechas también habla en hora de México"
        );

        let f = Filtro {
            desde: "2026-10-11".into(),
            hasta: "2026-10-11".into(),
            ..Default::default()
        };
        assert_eq!(buscar(db.conn(), &f).unwrap().len(), 0);
    }

    #[test]
    fn el_limite_se_respeta_y_por_defecto_trae_200() {
        let (db, id) = base();
        for i in 0..10 {
            anotar_usuario(&db, id, "venta", &format!("v{i}"));
        }
        let f = Filtro {
            limite: 3,
            ..Default::default()
        };
        assert_eq!(buscar(db.conn(), &f).unwrap().len(), 3);
        assert_eq!(buscar(db.conn(), &Filtro::default()).unwrap().len(), 10);
    }

    #[test]
    fn las_acciones_que_existen_se_pueden_listar() {
        let (db, id) = base();
        anotar_usuario(&db, id, "venta", "a");
        anotar_usuario(&db, id, "venta", "b");
        anotar_usuario(&db, id, "respaldo", "c");
        let a = acciones(db.conn()).unwrap();
        assert_eq!(a, vec!["respaldo".to_string(), "venta".to_string()]);
        assert_eq!(total(db.conn()).unwrap(), 3);
    }

    #[test]
    fn un_evento_sin_accion_no_se_guarda() {
        let (db, id) = base();
        assert!(registrar(db.conn(), Some(id), "", "sale", None, "").is_err());
        assert!(registrar(db.conn(), Some(id), "venta", "", None, "").is_err());
        assert_eq!(total(db.conn()).unwrap(), 0);
    }

    #[test]
    fn las_etiquetas_estan_en_espanol() {
        assert_eq!(etiqueta("venta"), "Venta");
        assert_eq!(etiqueta("autorizar"), "Autorización");
        assert_eq!(etiqueta("precio"), "Precio a mano");
        assert_eq!(etiqueta("descuento"), "Descuento");
        assert_eq!(etiqueta("fiado"), "Fiado");
        assert_eq!(etiqueta("diferencia"), "Corte que no cuadra");
        assert_eq!(etiqueta("retiro"), "Retiro o gasto");
        assert_eq!(etiqueta("pac_http"), "PAC por HTTP bloqueado");
        assert_eq!(etiqueta("impresion_prueba"), "Ticket de prueba");
        assert_eq!(
            etiqueta("impresion_prueba_fallida"),
            "Ticket de prueba fallido"
        );
        assert_eq!(etiqueta("cerrar_caja"), "Cortó caja");
        // Una acción desconocida se muestra tal cual, no se pierde.
        assert_eq!(etiqueta("loquesea"), "loquesea");
    }
}
