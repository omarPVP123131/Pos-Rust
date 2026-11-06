//! Respaldos automáticos con rotación.
//!
//! Un respaldo es la diferencia entre "se murió la computadora" y "perdimos el
//! mes". Este módulo hace la parte aburrida y confiable:
//!
//! - **Crea** la copia con `VACUUM INTO` (sale compacta y consistente).
//! - **Verifica** la copia antes de darla por buena: si el archivo no pasa el
//!   `integrity_check`, el respaldo se considera fallido, no "hecho".
//! - **Rota**: se conservan los últimos N y se borran los viejos, para que la
//!   memoria USB no se llene en silencio.
//! - **Encuentra dónde guardar**: detecta memorias USB y unidades montadas.
//!
//! Nada de esto borra la base original. Y nunca se borra el último respaldo
//! que queda, aunque la configuración pida "conservar 0".

use pos_core::{ahora_segs, FechaHora};
use rusqlite::OpenFlags;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Database, DbError, Result};

/// Prefijo de los archivos de respaldo.
pub const PREFIJO: &str = "respaldo-";
/// Cuántos respaldos se conservan si no se configura otra cosa.
pub const CONSERVAR_POR_DEFECTO: usize = 14;
/// Cada cuántos días se hace el respaldo si no se configura otra cosa.
pub const CADA_DIAS_POR_DEFECTO: i64 = 1;

/// Un respaldo ya hecho, tal como vive en el disco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RespaldoInfo {
    pub nombre: String,
    pub ruta: PathBuf,
    pub bytes: u64,
    /// Segundos desde el epoch de la última modificación del archivo.
    pub creado: i64,
}

/// Un lugar donde se puede guardar el respaldo (memoria USB, disco, carpeta).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unidad {
    pub etiqueta: String,
    pub ruta: PathBuf,
}

/// El estado de los respaldos: lo que la interfaz muestra en Configuración.
#[derive(Debug, Clone, Default)]
pub struct EstadoRespaldos {
    pub carpeta: String,
    pub total: usize,
    pub bytes: u64,
    pub ultimo: Option<RespaldoInfo>,
    /// `true` si ya toca hacer uno (no hay ninguno o el último es muy viejo).
    pub pendiente: bool,
}

fn marca(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Nombre del archivo para un momento dado: `respaldo-20261010-181500.db`.
/// El formato ordena bien alfabéticamente, así que listar y ordenar es lo mismo.
pub fn nombre_para(ahora: i64) -> String {
    format!("{PREFIJO}{}.db", FechaHora::cdmx(ahora).compacto())
}

/// Crea un respaldo en `carpeta` y lo verifica antes de devolverlo.
pub fn crear(db: &Database, carpeta: impl AsRef<Path>) -> Result<RespaldoInfo> {
    crear_en(db, carpeta.as_ref(), ahora_segs())
}

/// Igual que [`crear`] pero con el reloj inyectado (para pruebas y para no
/// chocar con un respaldo hecho en el mismo segundo).
pub fn crear_en(db: &Database, carpeta: &Path, ahora: i64) -> Result<RespaldoInfo> {
    let nombre = nombre_para(ahora);
    let mut ruta = carpeta.join(&nombre);
    // Si por casualidad ya existe (misma fecha, mismo segundo), se le pone un
    // sufijo en vez de fallar: `VACUUM INTO` exige que el destino no exista.
    let mut intento = 2;
    while ruta.exists() {
        ruta = carpeta.join(format!(
            "{PREFIJO}{}-{intento}.db",
            FechaHora::cdmx(ahora).compacto()
        ));
        intento += 1;
        if intento > 50 {
            return Err(DbError::Negocio(
                "Hay demasiados respaldos con la misma marca de tiempo".into(),
            ));
        }
    }
    db.respaldar(&ruta)?;

    // La copia se abre y se verifica: un respaldo que no sirve no se reporta
    // como terminado, se reporta como error.
    if !verificar(&ruta)? {
        let _ = fs::remove_file(&ruta);
        return Err(DbError::Negocio(format!(
            "El respaldo salió dañado y se descartó: {}",
            ruta.display()
        )));
    }
    info(&ruta).ok_or_else(|| {
        DbError::Negocio("El respaldo se hizo pero no lo puedo leer del disco".into())
    })
}

/// Abre una copia **en modo lectura** y le corre `integrity_check`.
/// Es la única forma honesta de saber si un respaldo sirve.
pub fn verificar(ruta: &Path) -> Result<bool> {
    if !ruta.exists() {
        return Err(DbError::Negocio(format!(
            "No existe el respaldo: {}",
            ruta.display()
        )));
    }
    // Si el archivo está tan dañado que no abre o no responde la consulta, la
    // respuesta es "no sirve", no un error: es exactamente lo que el cajero
    // necesita saber de ese respaldo.
    let Ok(copia) = rusqlite::Connection::open_with_flags(
        ruta,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Ok(false);
    };
    let Ok(valor) = copia.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)) else {
        return Ok(false);
    };
    Ok(valor.trim() == "ok")
}

fn info(ruta: &Path) -> Option<RespaldoInfo> {
    let meta = fs::metadata(ruta).ok()?;
    Some(RespaldoInfo {
        nombre: ruta.file_name()?.to_string_lossy().into_owned(),
        ruta: ruta.to_path_buf(),
        bytes: meta.len(),
        creado: marca(&meta),
    })
}

/// Todos los respaldos de la carpeta, **del más reciente al más viejo**.
pub fn listar(carpeta: &Path) -> Vec<RespaldoInfo> {
    let mut salida: Vec<RespaldoInfo> = match fs::read_dir(carpeta) {
        Ok(d) => d
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(PREFIJO) && n.ends_with(".db"))
            })
            .filter_map(|p| info(&p))
            .collect(),
        Err(_) => Vec::new(),
    };
    // Por nombre (que lleva la fecha) y además por marca de tiempo: así el
    // orden es correcto aunque alguien haya copiado archivos a mano.
    salida.sort_by(|a, b| b.nombre.cmp(&a.nombre).then(b.creado.cmp(&a.creado)));
    salida
}

/// El respaldo más reciente de la carpeta, si hay alguno.
pub fn ultimo(carpeta: &Path) -> Option<RespaldoInfo> {
    listar(carpeta).into_iter().next()
}

/// Borra los respaldos más viejos dejando sólo los últimos `conservar`.
/// Devuelve los nombres que se borraron. **Nunca deja la carpeta vacía** si
/// había al menos un respaldo: siempre se conserva el último.
pub fn rotar(carpeta: &Path, conservar: usize) -> Vec<String> {
    let todos = listar(carpeta);
    if todos.len() <= 1 {
        return Vec::new();
    }
    let conservar = conservar.max(1);
    let sobran = todos.iter().skip(conservar);
    let mut borrados = Vec::new();
    for viejo in sobran {
        // También se lleva el -wal/-shm si llegaran a existir.
        for extra in [
            viejo.ruta.clone(),
            viejo.ruta.with_extension("db-wal"),
            viejo.ruta.with_extension("db-shm"),
        ] {
            if extra.exists() {
                let _ = fs::remove_file(&extra);
            }
        }
        borrados.push(viejo.nombre.clone());
    }
    borrados
}

/// ¿Ya toca respaldar? Verdadero si no hay ninguno o si el más reciente tiene
/// más de `cada_dias` días. `cada_dias = 0` significa "cada vez que se pueda".
pub fn necesita(carpeta: &Path, cada_dias: i64, ahora: i64) -> bool {
    if cada_dias <= 0 {
        return true;
    }
    match ultimo(carpeta) {
        None => true,
        Some(u) => u.creado + cada_dias * 86_400 <= ahora,
    }
}

/// El estado completo, para mostralo en la pantalla de configuración.
pub fn estado(carpeta: &Path, cada_dias: i64, ahora: i64) -> EstadoRespaldos {
    let todos = listar(carpeta);
    EstadoRespaldos {
        carpeta: carpeta.display().to_string(),
        total: todos.len(),
        bytes: todos.iter().map(|r| r.bytes).sum(),
        ultimo: todos.into_iter().next(),
        pendiente: necesita(carpeta, cada_dias, ahora),
    }
}

/// Unidades donde se puede guardar el respaldo: memorias USB, discos
/// montados y, en Windows, las letras de unidad que existan.
///
/// No se usa ninguna dependencia externa: se revisan las rutas estándar de
/// cada sistema. Si una carpeta no se puede leer, simplemente no aparece.
pub fn unidades() -> Vec<Unidad> {
    let mut salida = Vec::new();
    let mut vista: Vec<PathBuf> = Vec::new();

    if cfg!(windows) {
        for letra in b'A'..=b'Z' {
            let raiz = PathBuf::from(format!("{}:\\", letra as char));
            if raiz.exists() {
                vista.push(raiz);
            }
        }
    } else {
        let usuario = std::env::var("USER").unwrap_or_default();
        let mut bases = vec![
            PathBuf::from("/media"),
            PathBuf::from("/run/media"),
            PathBuf::from("/mnt"),
            PathBuf::from("/Volumes"),
        ];
        if !usuario.is_empty() {
            bases.push(PathBuf::from(format!("/media/{usuario}")));
            bases.push(PathBuf::from(format!("/run/media/{usuario}")));
        }
        for base in bases {
            if let Ok(dir) = fs::read_dir(&base) {
                for entrada in dir.filter_map(|e| e.ok()) {
                    let ruta = entrada.path();
                    if ruta.is_dir() && !vista.contains(&ruta) {
                        vista.push(ruta);
                    }
                }
            }
        }
    }

    for ruta in vista {
        let etiqueta = ruta
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| ruta.display().to_string());
        salida.push(Unidad { etiqueta, ruta });
    }
    salida.sort_by(|a, b| a.etiqueta.cmp(&b.etiqueta));
    salida.dedup_by(|a, b| a.ruta == b.ruta);
    salida
}

/// Carpeta sugerida si nadie configuró una: junto a la base de datos.
pub fn carpeta_sugerida(db_path: &Path) -> PathBuf {
    let padre = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    padre.join("respaldos")
}

/// Segundos desde el epoch, por si alguien quiere el reloj desde `pos-db`.
pub fn ahora() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use std::fs;
    use std::time::UNIX_EPOCH;

    fn base() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// Pone la fecha de modificación del archivo (para probar la rotación y el
    /// "¿ya toca respaldar?" sin esperar días reales).
    fn fechar(ruta: &Path, segs: i64) {
        let f = fs::OpenOptions::new().write(true).open(ruta).unwrap();
        f.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(segs as u64))
            .unwrap();
    }

    #[test]
    fn el_nombre_lleva_la_fecha_y_ordena_bien() {
        let a = nombre_para(20_736 * 86_400 + 6 * 3600);
        let b = nombre_para(20_737 * 86_400 + 6 * 3600);
        assert_eq!(a, "respaldo-20261010-000000.db");
        assert_eq!(b, "respaldo-20261011-000000.db");
        assert!(a < b, "los nombres deben ordenarse por fecha");
    }

    #[test]
    fn crear_deja_un_respaldo_verificado_y_legible() {
        let db = base();
        let d = dir();
        let r = crear_en(&db, d.path(), 20_736 * 86_400 + 6 * 3600).unwrap();
        assert!(r.ruta.exists());
        assert!(r.bytes > 0);
        assert!(verificar(&r.ruta).unwrap());

        // Y la copia es una base de datos de verdad: se puede abrir y leer.
        let copia = Database::open(&r.ruta).unwrap();
        assert!(copia.integrity_ok().unwrap());
    }

    #[test]
    fn dos_respaldos_en_el_mismo_segundo_no_se_pisan() {
        let db = base();
        let d = dir();
        let a = crear_en(&db, d.path(), 20_736 * 86_400).unwrap();
        let b = crear_en(&db, d.path(), 20_736 * 86_400).unwrap();
        assert_ne!(a.nombre, b.nombre);
        assert_eq!(listar(d.path()).len(), 2);
    }

    #[test]
    fn listar_ordena_del_mas_reciente_al_mas_viejo() {
        let db = base();
        let d = dir();
        for dia in [10i64, 11, 12] {
            crear_en(&db, d.path(), (20_730 + dia) * 86_400).unwrap();
        }
        let todos = listar(d.path());
        assert_eq!(todos.len(), 3);
        assert!(todos[0].nombre > todos[1].nombre);
        assert!(todos[1].nombre > todos[2].nombre);
        assert_eq!(ultimo(d.path()).unwrap().nombre, todos[0].nombre);
    }

    #[test]
    fn la_rotacion_conserva_los_ultimos_y_borra_los_viejos() {
        let db = base();
        let d = dir();
        for dia in 0..6 {
            crear_en(&db, d.path(), (20_700 + dia) * 86_400).unwrap();
        }
        assert_eq!(listar(d.path()).len(), 6);

        let borrados = rotar(d.path(), 3);
        assert_eq!(borrados.len(), 3);
        let quedan = listar(d.path());
        assert_eq!(quedan.len(), 3);
        // Se quedaron los tres más nuevos.
        assert_eq!(quedan[0].nombre, nombre_para((20_700 + 5) * 86_400));
        assert_eq!(quedan[2].nombre, nombre_para((20_700 + 3) * 86_400));
    }

    #[test]
    fn la_rotacion_nunca_deja_la_carpeta_vacia() {
        let db = base();
        let d = dir();
        crear_en(&db, d.path(), 20_700 * 86_400).unwrap();
        assert!(rotar(d.path(), 0).is_empty());
        assert_eq!(listar(d.path()).len(), 1);
        // Con una carpeta vacía tampoco truena.
        assert!(rotar(d.path(), 5).is_empty());
    }

    #[test]
    fn un_respaldo_daniado_se_detecta() {
        let db = base();
        let d = dir();
        let r = crear_en(&db, d.path(), 20_700 * 86_400).unwrap();

        // Se le escribe basura encima, como pasaría en una memoria barrida.
        let mut contenido = fs::read(&r.ruta).unwrap();
        for byte in contenido.iter_mut().take(200).skip(100) {
            *byte = 0;
        }
        fs::write(&r.ruta, &contenido).unwrap();

        assert!(
            !verificar(&r.ruta).unwrap(),
            "un respaldo alterado debe fallar la verificación"
        );
    }

    #[test]
    fn verificar_un_archivo_que_no_existe_es_un_error_claro() {
        let d = dir();
        let falta = d.path().join("respaldo-que-no-existe.db");
        let e = verificar(&falta).unwrap_err();
        assert!(e.to_string().contains("No existe"));
    }

    #[test]
    fn necesita_respaldo_segun_los_dias() {
        let db = base();
        let d = dir();
        let ahora = 20_700 * 86_400;

        // Sin ningún respaldo, siempre hace falta.
        assert!(necesita(d.path(), 1, ahora));

        let r = crear_en(&db, d.path(), ahora).unwrap();
        fechar(&r.ruta, ahora);
        // Recién hecho: no hace falta.
        assert!(!necesita(d.path(), 1, ahora));
        // Doce horas después: tampoco.
        assert!(!necesita(d.path(), 1, ahora + 12 * 3600));
        // Dos días después: sí.
        assert!(necesita(d.path(), 1, ahora + 2 * 86_400));
        // Con "cada 7 días", a los dos días no toca pero a los ocho sí.
        assert!(!necesita(d.path(), 7, ahora + 2 * 86_400));
        assert!(necesita(d.path(), 7, ahora + 8 * 86_400));
        // "cada 0 días" = siempre que se pueda.
        assert!(necesita(d.path(), 0, ahora));
    }

    #[test]
    fn el_estado_resume_lo_que_ve_la_interfaz() {
        let db = base();
        let d = dir();
        let ahora = 20_700 * 86_400;
        let e = estado(d.path(), 1, ahora);
        assert_eq!(e.total, 0);
        assert!(e.pendiente);

        crear_en(&db, d.path(), ahora).unwrap();
        crear_en(&db, d.path(), ahora + 86_400).unwrap();
        let e = estado(d.path(), 1, ahora + 86_400);
        assert_eq!(e.total, 2);
        assert_eq!(
            e.bytes,
            listar(d.path()).iter().map(|r| r.bytes).sum::<u64>()
        );
        assert!(!e.pendiente);
        assert!(e.ultimo.is_some());
        assert_eq!(e.carpeta, d.path().display().to_string());
    }

    #[test]
    fn la_carpeta_sugerida_va_junto_a_la_base() {
        assert_eq!(
            carpeta_sugerida(Path::new("/home/cajero/.local/share/pos/tienda.db")),
            PathBuf::from("/home/cajero/.local/share/pos/respaldos")
        );
        // Con una ruta relativa no se rompe.
        assert_eq!(
            carpeta_sugerida(Path::new("tienda.db")),
            PathBuf::from("./respaldos")
        );
    }

    #[test]
    fn las_unidades_se_listan_sin_tronar() {
        // No se puede asegurar que haya una USB montada, pero sí que no explota
        // y que no devuelve rutas repetidas.
        let u = unidades();
        let mut rutas: Vec<&PathBuf> = u.iter().map(|x| &x.ruta).collect();
        let antes = rutas.len();
        rutas.sort();
        rutas.dedup();
        assert_eq!(antes, rutas.len(), "no debe haber unidades repetidas");
        for unidad in &u {
            assert!(!unidad.etiqueta.is_empty());
        }
    }
}
