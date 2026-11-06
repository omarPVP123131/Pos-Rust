//! Usuarios y autenticación por PIN.
//!
//! El PIN se guarda con **Argon2id** y sal aleatoria; nunca en texto plano.
//! Un PIN de 4 dígitos tiene poca entropía, así que el control real es el
//! bloqueo temporal tras varios intentos fallidos (y el sentido común:
//! no uses 1234 en producción).

use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rusqlite::{params, Connection, OptionalExtension};

use crate::{DbError, Result};
use pos_core::{Permiso, Pin, Role, User, MAX_INTENTOS_PIN};

pub struct Users;

impl Users {
    /// Si la base no tiene usuarios, crea el administrador inicial.
    /// Devuelve `Some(id)` cuando lo creó (para avisarle al usuario que cambie
    /// el PIN en la primer sesión).
    pub fn asegurar_admin(conn: &mut Connection, pin_inicial: &str) -> Result<Option<i64>> {
        let cuenta: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        if cuenta > 0 {
            return Ok(None);
        }
        let pin = Pin::parse(pin_inicial)?;
        let id = Self::crear(conn, "admin", "Administrador", pin.as_str(), Role::Admin)?;
        Ok(Some(id))
    }

    pub fn crear(
        conn: &mut Connection,
        username: &str,
        display_name: &str,
        pin: &str,
        role: Role,
    ) -> Result<i64> {
        let _ = Pin::parse(pin)?;
        let hash = hash_pin(pin)?;
        conn.execute(
            "INSERT INTO users (username, display_name, pin_hash, role)
             VALUES (?1, ?2, ?3, ?4)",
            params![username.trim(), display_name.trim(), hash, role.as_str()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list(conn: &Connection) -> Result<Vec<User>> {
        let mut stmt = conn.prepare(
            "SELECT id, username, display_name, role, is_active, failed_attempts
             FROM users WHERE is_active = 1 ORDER BY display_name",
        )?;
        let filas = stmt
            .query_map([], |r| {
                Ok(User {
                    id: r.get(0)?,
                    username: r.get(1)?,
                    display_name: r.get(2)?,
                    role: Role::desde_texto(&r.get::<_, String>(3)?),
                    is_active: r.get::<_, i32>(4)? != 0,
                    failed_attempts: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filas)
    }

    pub fn by_id(conn: &Connection, id: i64) -> Result<Option<User>> {
        conn.query_row(
            "SELECT id, username, display_name, role, is_active, failed_attempts
             FROM users WHERE id = ?1",
            params![id],
            |r| {
                Ok(User {
                    id: r.get(0)?,
                    username: r.get(1)?,
                    display_name: r.get(2)?,
                    role: Role::desde_texto(&r.get::<_, String>(3)?),
                    is_active: r.get::<_, i32>(4)? != 0,
                    failed_attempts: r.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    /// Verifica PIN. Cuenta los intentos fallidos y bloquea al llegar al tope.
    pub fn autenticar(conn: &mut Connection, username: &str, pin: &str) -> Result<User> {
        let user = Self::by_username(conn, username)?
            .ok_or(DbError::Negocio("Usuario o PIN incorrecto".into()))?;

        if !user.is_active {
            return Err(DbError::Negocio("Ese usuario está dado de baja".into()));
        }
        if user.bloqueado(MAX_INTENTOS_PIN) {
            return Err(DbError::Negocio(
                pos_core::CoreError::UsuarioBloqueado.to_string(),
            ));
        }

        let hash: String = conn.query_row(
            "SELECT pin_hash FROM users WHERE id = ?1",
            params![user.id],
            |r| r.get(0),
        )?;

        if verificar_pin(pin, &hash) {
            conn.execute(
                "UPDATE users SET failed_attempts = 0 WHERE id = ?1",
                params![user.id],
            )?;
            conn.execute(
                "INSERT INTO audit_log (user_id, action, entity, entity_id)
                 VALUES (?1, 'login', 'user', ?2)",
                params![user.id, user.id],
            )?;
            Ok(User {
                failed_attempts: 0,
                ..user
            })
        } else {
            conn.execute(
                "UPDATE users SET failed_attempts = failed_attempts + 1 WHERE id = ?1",
                params![user.id],
            )?;
            let intentos = user.failed_attempts + 1;
            let restantes = (MAX_INTENTOS_PIN - intentos).max(0);
            // Un PIN equivocado también se anota: si alguien está intentando
            // entrar con la cuenta de otro, se tiene que poder ver.
            let _ = conn.execute(
                "INSERT INTO audit_log (user_id, action, entity, entity_id, payload_json)
                 VALUES (?1, 'login_fallido', 'user', ?2, ?3)",
                params![
                    user.id,
                    user.id,
                    format!("intento {intentos} de {MAX_INTENTOS_PIN}")
                ],
            );
            if restantes == 0 {
                let _ = conn.execute(
                    "INSERT INTO audit_log (user_id, action, entity, entity_id, payload_json)
                     VALUES (?1, 'bloqueo', 'user', ?2, ?3)",
                    params![
                        user.id,
                        user.id,
                        format!("{} se quedó sin intentos", user.username)
                    ],
                );
                return Err(DbError::Negocio(
                    pos_core::CoreError::UsuarioBloqueado.to_string(),
                ));
            }
            Err(DbError::Negocio(format!(
                "PIN incorrecto. Te quedan {restantes} intentos."
            )))
        }
    }

    /// Verifica credenciales **sin** contar intentos: sirve para autorizar una
    /// operación puntual (descuento, retiro), no para iniciar sesión.
    pub fn verificar(conn: &Connection, username: &str, pin: &str) -> Result<User> {
        let user = Self::by_username(conn, username)?
            .ok_or(DbError::Negocio("Usuario o PIN incorrecto".into()))?;
        if !user.is_active {
            return Err(DbError::Negocio("Ese usuario está dado de baja".into()));
        }
        let hash: String = conn.query_row(
            "SELECT pin_hash FROM users WHERE id = ?1",
            params![user.id],
            |r| r.get(0),
        )?;
        if verificar_pin(pin, &hash) {
            Ok(user)
        } else {
            Err(DbError::Negocio("Usuario o PIN incorrecto".into()))
        }
    }

    pub fn by_username(conn: &Connection, username: &str) -> Result<Option<User>> {
        conn.query_row(
            "SELECT id, username, display_name, role, is_active, failed_attempts
             FROM users WHERE username = ?1",
            params![username.trim()],
            |r| {
                Ok(User {
                    id: r.get(0)?,
                    username: r.get(1)?,
                    display_name: r.get(2)?,
                    role: Role::desde_texto(&r.get::<_, String>(3)?),
                    is_active: r.get::<_, i32>(4)? != 0,
                    failed_attempts: r.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn cambiar_pin(conn: &Connection, id: i64, nuevo_pin: &str) -> Result<()> {
        let _ = Pin::parse(nuevo_pin)?;
        let hash = hash_pin(nuevo_pin)?;
        conn.execute(
            "UPDATE users SET pin_hash = ?1, failed_attempts = 0 WHERE id = ?2",
            params![hash, id],
        )?;
        Ok(())
    }

    /// Desbloquea al usuario (lo hace un supervisor o el admin).
    pub fn desbloquear(conn: &Connection, id: i64) -> Result<()> {
        conn.execute(
            "UPDATE users SET failed_attempts = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn desactivar(conn: &Connection, id: i64) -> Result<()> {
        conn.execute("UPDATE users SET is_active = 0 WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Nombre y rol. El PIN va por [`cambiar_pin`].
    pub fn actualizar(conn: &Connection, id: i64, display_name: &str, role: Role) -> Result<()> {
        let nombre = display_name.trim();
        if nombre.chars().count() < 2 {
            return Err(DbError::Negocio(
                "El nombre tiene que tener al menos dos letras.".into(),
            ));
        }
        conn.execute(
            "UPDATE users SET display_name = ?1, role = ?2 WHERE id = ?3",
            params![nombre, role.as_str(), id],
        )?;
        Ok(())
    }

    /// Cuántos administradores activos hay. No se puede dejar la tienda sin uno.
    pub fn admins_activos(conn: &Connection) -> Result<i64> {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE is_active = 1 AND role = 'admin'",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// Atajo para las validaciones de la UI.
    pub fn puede(conn: &Connection, id: i64, permiso: Permiso) -> bool {
        Self::by_id(conn, id)
            .ok()
            .flatten()
            .map(|u| u.puede(permiso))
            .unwrap_or(false)
    }
}

fn hash_pin(pin: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map_err(|e| DbError::Negocio(format!("No se pudo generar el hash: {e}")))?;
    Ok(hash.to_string())
}

fn verificar_pin(pin: &str, hash_guardado: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash_guardado) else {
        return false;
    };
    Argon2::default()
        .verify_password(pin.as_bytes(), &parsed)
        .is_ok()
}

impl From<pos_core::CoreError> for DbError {
    fn from(e: pos_core::CoreError) -> Self {
        DbError::Negocio(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn si_no_hay_usuarios_crea_el_admin() {
        let mut db = db();
        assert!(Users::asegurar_admin(db.conn_mut(), "1234")
            .unwrap()
            .is_some());
        // La segunda vez no duplica.
        assert!(Users::asegurar_admin(db.conn_mut(), "1234")
            .unwrap()
            .is_none());
        assert_eq!(Users::list(db.conn()).unwrap().len(), 1);
    }

    #[test]
    fn autentica_con_el_pin_correcto_y_rechaza_el_malo() {
        let mut db = db();
        Users::crear(db.conn_mut(), "ana", "Ana", "4321", Role::Cajero).unwrap();

        let ana = Users::autenticar(db.conn_mut(), "ana", "4321").unwrap();
        assert_eq!(ana.display_name, "Ana");
        assert_eq!(ana.role, Role::Cajero);

        assert!(Users::autenticar(db.conn_mut(), "ana", "0000").is_err());
        assert!(Users::autenticar(db.conn_mut(), "nadie", "4321").is_err());
    }

    #[test]
    fn bloquea_al_usuario_tras_varios_intentos() {
        let mut db = db();
        Users::crear(db.conn_mut(), "ana", "Ana", "4321", Role::Cajero).unwrap();

        for _ in 0..MAX_INTENTOS_PIN {
            let _ = Users::autenticar(db.conn_mut(), "ana", "0000");
        }
        // Ya bloqueado: ni con el PIN correcto entra.
        assert!(Users::autenticar(db.conn_mut(), "ana", "4321").is_err());

        let id = Users::by_username(db.conn(), "ana").unwrap().unwrap().id;
        Users::desbloquear(db.conn(), id).unwrap();
        assert!(Users::autenticar(db.conn_mut(), "ana", "4321").is_ok());
    }

    #[test]
    fn el_pin_nunca_se_guarda_en_texto_plano() {
        let mut db = db();
        Users::crear(db.conn_mut(), "ana", "Ana", "4321", Role::Cajero).unwrap();
        let hash: String = db
            .conn()
            .query_row(
                "SELECT pin_hash FROM users WHERE username = 'ana'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!hash.contains("4321"));
        assert!(hash.starts_with("$argon2"));
    }

    #[test]
    fn los_permisos_se_consultan_desde_la_base() {
        let mut db = db();
        Users::crear(db.conn_mut(), "ana", "Ana", "4321", Role::Cajero).unwrap();
        Users::crear(db.conn_mut(), "jefe", "El Jefe", "1111", Role::Admin).unwrap();
        let ana = Users::by_username(db.conn(), "ana").unwrap().unwrap();
        let jefe = Users::by_username(db.conn(), "jefe").unwrap().unwrap();

        assert!(!Users::puede(db.conn(), ana.id, Permiso::Descuento));
        assert!(Users::puede(db.conn(), jefe.id, Permiso::Descuento));
        assert!(Users::puede(db.conn(), jefe.id, Permiso::CerrarCaja));
    }

    #[test]
    fn se_puede_cambiar_el_pin_y_dar_de_baja() {
        let mut db = db();
        Users::crear(db.conn_mut(), "ana", "Ana", "4321", Role::Cajero).unwrap();
        let id = Users::by_username(db.conn(), "ana").unwrap().unwrap().id;

        Users::cambiar_pin(db.conn(), id, "8765").unwrap();
        assert!(Users::autenticar(db.conn_mut(), "ana", "4321").is_err());
        assert!(Users::autenticar(db.conn_mut(), "ana", "8765").is_ok());

        Users::desactivar(db.conn(), id).unwrap();
        assert!(Users::autenticar(db.conn_mut(), "ana", "8765").is_err());
    }
}
