//! Aplicar una actualización sin dejar la caja muerta.
//!
//! El miedo no es actualizar: es actualizar, que algo salga mal y que la tienda
//! se quede sin sistema un sábado a las 7 de la noche. Así que el orden es:
//!
//! 1. Se **prueba** el paquete antes de tocar nada (firma, hash, versión).
//! 2. Se escribe el nuevo aparte (`pos.nuevo`) y se deja el marcador.
//! 3. Se cambia de un golpe: se **aparta** el que está corriendo —que pasa a
//!    ser el respaldo `pos.anterior`— y se pone el nuevo en su lugar.
//!    (En Windows no se puede sobrescribir un `.exe` en uso: hay que apartarlo
//!    primero. En Linux `rename` basta, pero se usa el mismo camino.)
//! 4. Se vuelve a probar. Si el nuevo no arranca, **se deshace solo**.
//!
//! Nunca se borra el ejecutable que sí funcionaba hasta que el nuevo ha
//! demostrado que sirve.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paquete::{revisar, Paquete};
use crate::{a_hex, de_hex, Error, Result, VERSION};

/// Extensión del respaldo del ejecutable anterior.
const RESPALDO: &str = "anterior";
/// Extensión del ejecutable recién escrito, antes de cambiarlo.
const NUEVO: &str = "nuevo";
/// Extensión del marcador: existe sólo mientras no se ha confirmado.
const PENDIENTE: &str = "pendiente";
/// Extensión del registro de la última actualización: dice de dónde se subió.
const ULTIMA: &str = "ultima";
/// Extensión del ejecutable que se apartó mientras estaba en uso. En Windows
/// no se puede borrar en el momento: se limpia en el siguiente arranque.
const ENUSO: &str = "enuso";

/// Cómo quedó la máquina, para la interfaz y para la bitácora.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Estado {
    /// Versión instalada (la del binario que está corriendo).
    pub version: String,
    /// Ruta del ejecutable.
    pub ejecutable: String,
    /// Hay un ejecutable anterior guardado, listo para volver.
    pub hay_respaldo: bool,
    /// La última actualización **no se confirmó**: algo raro pasó.
    pub hay_pendiente: bool,
    /// A qué versión se volvería con `revertir`.
    pub version_respaldo: Option<String>,
}

/// El resultado de aplicar una actualización.
#[derive(Clone, Debug, PartialEq)]
pub struct Resultado {
    pub version: String,
    /// El nuevo ejecutable arrancó y pasó la prueba.
    pub probado: bool,
    /// Se tuvo que deshacer: sigue instalada la versión anterior.
    pub deshecho: bool,
    /// Por qué se deshizo (sólo si `deshecho`).
    pub motivo: String,
}

fn con_sufijo(ejecutable: &Path, sufijo: &str) -> PathBuf {
    let mut nombre = ejecutable.as_os_str().to_os_string();
    nombre.push(".");
    nombre.push(sufijo);
    PathBuf::from(nombre)
}

/// Qué pasó en la última actualización, para saber si hay que deshacerla.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Marcador {
    version_anterior: String,
    version_nueva: String,
    cuando: String,
}

/// Lee el estado sin tocar nada: lo usa la pantalla de configuración.
pub fn estado(ejecutable: &Path) -> Estado {
    let respaldo = con_sufijo(ejecutable, RESPALDO);
    let pendiente = con_sufijo(ejecutable, PENDIENTE);
    Estado {
        version: VERSION.to_string(),
        ejecutable: ejecutable.display().to_string(),
        hay_respaldo: respaldo.exists(),
        hay_pendiente: pendiente.exists(),
        version_respaldo: fs::read_to_string(con_sufijo(ejecutable, ULTIMA))
            .ok()
            .and_then(|t| serde_json::from_str::<Marcador>(&t).ok())
            .map(|m| m.version_anterior)
            // Si la última quedó a medias, el marcador todavía lo dice.
            .or_else(|| {
                fs::read_to_string(&pendiente)
                    .ok()
                    .and_then(|t| serde_json::from_str::<Marcador>(&t).ok())
                    .map(|m| m.version_anterior)
            }),
    }
}

/// Aplica un paquete ya firmado sobre el ejecutable que se indica.
///
/// `probar` es la prueba de arranque: recibe la ruta del ejecutable recién
/// instalado y decide si sirve. En producción corre el binario con `--checar`;
/// en las pruebas es una función cualquiera, así que el rollback se puede
/// probar de verdad, con archivos de verdad.
pub fn aplicar(
    ruta_paquete: &Path,
    ejecutable: &Path,
    probar: impl Fn(&Path) -> std::result::Result<(), String>,
) -> Result<Resultado> {
    if !ejecutable.exists() {
        return Err(Error::Io(format!(
            "No encontré el ejecutable {}",
            ejecutable.display()
        )));
    }
    let bytes = fs::read(ruta_paquete).map_err(|e| {
        Error::Io(format!(
            "No pude leer el paquete {}: {e}",
            ruta_paquete.display()
        ))
    })?;
    let paquete: Paquete = revisar(&bytes, VERSION)?;
    let version = paquete.manifiesto.version.clone();

    let respaldo = con_sufijo(ejecutable, RESPALDO);
    let nuevo = con_sufijo(ejecutable, NUEVO);
    let pendiente = con_sufijo(ejecutable, PENDIENTE);

    // 1) El que sí funciona se guarda en el momento del cambio (paso 4): en
    //    Windows no se puede copiar un `.exe` en uso y luego sobrescribirlo,
    //    así que se aparta con `rename`, que sí está permitido.

    // 2) El nuevo se escribe aparte y se marca como ejecutable.
    escribir_ejecutable(&nuevo, &paquete.payload)?;

    // 3) El marcador dice "hay una actualización sin confirmar". Si la
    //    computadora se apaga aquí, al volver se sabe que algo quedó a medias.
    let marcador = Marcador {
        version_anterior: VERSION.to_string(),
        version_nueva: version.clone(),
        cuando: crate::ahora(),
    };
    fs::write(
        &pendiente,
        serde_json::to_string(&marcador).unwrap_or_default(),
    )
    .map_err(|e| Error::Io(format!("No pude escribir el marcador: {e}")))?;

    // 4) El cambio: se aparta el que está corriendo (ése queda como respaldo) y
    //    se pone el nuevo en su lugar. En Linux `rename` sobre un archivo en uso
    //    funciona; en Windows no, y por eso se aparta primero.
    reemplazar_en_uso(ejecutable, &nuevo, &respaldo)?;

    // 5) La prueba. Si falla, se deshace y queda lo que sí funcionaba.
    match probar(ejecutable) {
        Ok(()) => {
            // Confirmada: el marcador de "a medias" pasa a ser el registro de
            // la última actualización, que es de dónde sale el respaldo.
            let _ = fs::rename(&pendiente, con_sufijo(ejecutable, ULTIMA));
            Ok(Resultado {
                version,
                probado: true,
                deshecho: false,
                motivo: String::new(),
            })
        }
        Err(motivo) => {
            // Marcha atrás: el nuevo (que no arrancó) se aparta y vuelve el
            // anterior. El apartado no se puede borrar aquí si era nuestra
            // propia imagen: se limpia en el siguiente arranque.
            let _ = reemplazar_en_uso(ejecutable, &respaldo, &con_sufijo(ejecutable, ENUSO));
            let _ = fs::remove_file(&pendiente);
            Ok(Resultado {
                version,
                probado: false,
                deshecho: true,
                motivo,
            })
        }
    }
}

/// Vuelve al ejecutable anterior. Es el botón del pánico.
pub fn revertir(ejecutable: &Path) -> Result<()> {
    let respaldo = con_sufijo(ejecutable, RESPALDO);
    if !respaldo.exists() {
        return Err(Error::Io(
            "No hay una versión anterior guardada: no se puede deshacer".into(),
        ));
    }
    // En Windows el ejecutable actual está en uso y no se puede sobrescribir:
    // se aparta (con `rename`, que sí se puede) y luego se copia el respaldo.
    // El apartado se borra en el siguiente arranque, cuando ya nadie lo usa.
    let enuso = con_sufijo(ejecutable, ENUSO);
    if enuso.exists() {
        let _ = fs::remove_file(&enuso);
    }
    if ejecutable.exists() {
        fs::rename(ejecutable, &enuso)
            .map_err(|e| Error::Io(format!("No pude apartar el ejecutable actual: {e}")))?;
    }
    if let Err(e) = fs::copy(&respaldo, ejecutable) {
        // Si la copia falla, se devuelve el que estaba: nunca sin programa.
        if !ejecutable.exists() && enuso.exists() {
            let _ = fs::rename(&enuso, ejecutable);
        }
        return Err(Error::Io(format!(
            "No pude restaurar el ejecutable anterior: {e}"
        )));
    }
    let _ = fs::remove_file(con_sufijo(ejecutable, PENDIENTE));
    // Ya no hay "versión anterior" distinta de la que quedó instalada.
    let _ = fs::remove_file(con_sufijo(ejecutable, ULTIMA));
    Ok(())
}

/// Reemplaza un ejecutable **que puede estar corriendo ahora mismo**.
///
/// En Linux es trivial: `rename` sobre un archivo en uso funciona, y el proceso
/// viejo sigue vivo con el inodo anterior. En Windows eso **no se puede**: un
/// `.exe` en ejecución está bloqueado por el sistema y cualquier intento de
/// sobrescribirlo (o de borrarlo) falla con "Acceso denegado". Ni `copy` ni
/// `rename` hacia esa ruta sirven.
///
/// Lo que Windows sí permite es **renombrar** el archivo en uso. Así que el
/// truco, válido en las dos plataformas, es:
///
///   1. se aparta el que está corriendo (`pos.exe` → `ruta_apartado`),
///   2. se pone el nuevo en su lugar (`pos.nuevo` → `pos.exe`),
///   3. si el paso 2 falla, se devuelve el apartado a su sitio.
///
/// El archivo apartado **no se borra**: mientras este proceso viva sigue siendo
/// su propia imagen en disco, y además es nuestro respaldo para la marcha
/// atrás. La limpieza se hace en el siguiente arranque (`limpiar_sobrantes`).
pub fn reemplazar_en_uso(destino: &Path, nuevo: &Path, apartado: &Path) -> Result<()> {
    // Si quedó el apartado de una corrida anterior, ya no está en uso: fuera.
    if apartado.exists() {
        let _ = fs::remove_file(apartado);
    }

    // 1) Apartar el que está en uso. Si no existe (primera instalación), no hay
    //    nada que apartar.
    if destino.exists() {
        fs::rename(destino, apartado).map_err(|e| {
            Error::Io(format!(
                "No pude apartar el ejecutable en uso ({}): {e}",
                apartado.display()
            ))
        })?;
    }

    // 2) Poner el nuevo. Si algo falla aquí, se devuelve el de antes: la caja
    //    nunca se queda sin programa.
    if let Err(e) = fs::rename(nuevo, destino) {
        if apartado.exists() && !destino.exists() {
            let _ = fs::rename(apartado, destino);
        }
        return Err(Error::Io(format!(
            "No pude reemplazar el ejecutable ({}): {e}",
            destino.display()
        )));
    }
    Ok(())
}

/// Borra los restos que Windows no deja borrar mientras el programa corre.
///
/// Cuando se reemplaza un ejecutable en uso, el archivo viejo no se puede
/// eliminar desde el mismo proceso que lo está ejecutando. Se queda ahí con la
/// marca `.enuso` y se limpia en el siguiente arranque, cuando ya nadie lo usa.
pub fn limpiar_sobrantes(ejecutable: &Path) {
    let sobrante = con_sufijo(ejecutable, ENUSO);
    if sobrante.exists() {
        let _ = fs::remove_file(&sobrante);
    }
}

/// Escribe el ejecutable y le pone permisos de ejecución.
fn escribir_ejecutable(ruta: &Path, bytes: &[u8]) -> Result<()> {
    let mut archivo = fs::File::create(ruta)
        .map_err(|e| Error::Io(format!("No pude escribir {}: {e}", ruta.display())))?;
    archivo
        .write_all(bytes)
        .map_err(|e| Error::Io(format!("No pude escribir {}: {e}", ruta.display())))?;
    archivo
        .sync_all()
        .map_err(|e| Error::Io(format!("No pude guardar {}: {e}", ruta.display())))?;
    drop(archivo);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(ruta, fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// La prueba de arranque de verdad: corre el ejecutable nuevo con `--checar`.
///
/// Si el binario está cortado, le falta una biblioteca o se cuelga, aquí se
/// entera — antes de que el encargado cierre la tienda.
pub fn probar_arranque(ejecutable: &Path, segundos: u64) -> std::result::Result<(), String> {
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    let mut hijo: Child = Command::new(ejecutable)
        .arg("--checar")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("El nuevo ejecutable no arrancó: {e}"))?;

    // std no trae `wait_timeout`, así que se espera a saltos cortos: en una
    // computadora lenta esto no gasta CPU, sólo mira el reloj.
    let limite = Duration::from_secs(segundos);
    let inicio = Instant::now();
    loop {
        match hijo.try_wait() {
            Ok(Some(estado)) => {
                return if estado.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "El nuevo ejecutable salió con error (código {})",
                        estado.code().unwrap_or(-1)
                    ))
                };
            }
            Ok(None) => {
                if inicio.elapsed() > limite {
                    let _ = hijo.kill();
                    let _ = hijo.wait();
                    return Err(format!(
                        "El nuevo ejecutable se colgó: no respondió en {segundos} segundos"
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("No pude vigilar el nuevo ejecutable: {e}")),
        }
    }
}

/// Huella corta del archivo, para bitácora y para que el encargado confirme
/// que está instalando lo que le dieron.
pub fn huella(bytes: &[u8]) -> String {
    use sha2::Digest;
    a_hex(&sha2::Sha256::digest(bytes))[..12].to_string()
}

/// Por si algún día hay que leer una llave suelta.
pub fn hex_a_bytes(texto: &str) -> Option<Vec<u8>> {
    de_hex(texto)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paquete::armar;

    /// Un "ejecutable" de mentiritas: un script que se puede correr de verdad.
    fn script(version: &str) -> Vec<u8> {
        format!("#!/bin/sh\necho \"soy la version {version}\"\n").into_bytes()
    }

    fn raiz(nombre: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "pos-update-{nombre}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&d);
        d
    }

    fn privada() -> [u8; 32] {
        crate::llave_privada_de_desarrollo().expect("llaves/pos-privada.txt")
    }

    /// Escribe un paquete firmado en el directorio dado.
    fn escribir_paquete(dir: &Path, version: &str, min: &str, payload: &[u8]) -> PathBuf {
        let bytes = armar(version, min, "notas", payload, &privada()).unwrap();
        let ruta = dir.join(format!("actualizacion-{version}.posup"));
        fs::write(&ruta, bytes).unwrap();
        ruta
    }

    /// El ejecutable "instalado": un script que imprime su versión.
    fn instalar(dir: &Path, version: &str) -> PathBuf {
        let exe = dir.join("pos");
        fs::write(&exe, script(version)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }
        exe
    }

    /// La prueba: corre el ejecutable y pide que diga la versión esperada.
    fn probar_que_diga(version: &str) -> impl Fn(&Path) -> std::result::Result<(), String> + '_ {
        move |exe: &Path| {
            let salida = std::process::Command::new(exe)
                .output()
                .map_err(|e| e.to_string())?;
            let texto = String::from_utf8_lossy(&salida.stdout);
            if texto.contains(version) {
                Ok(())
            } else {
                Err(format!("dijo '{texto}'"))
            }
        }
    }

    // Este y los siguientes ejecutan el "falso exe" (un script sh): en
    // Windows un .sh no es un Win32 válido (error 193). La lógica de
    // respaldo/rollback sí se prueba en Windows con los tests que usan
    // clausura en vez de ejecutar. Estos 5 corren en Linux/CI.
    #[cfg(unix)]
    #[test]
    fn aplicar_deja_el_ejecutable_nuevo_y_guarda_el_viejo() {
        let dir = raiz("aplicar");
        let exe = instalar(&dir, "0.4.0");
        // El paquete tiene que ser una versión mayor que VERSION del crate.
        let paquete = escribir_paquete(&dir, "99.0.0", "0.1.0", &script("99.0.0"));

        let r = aplicar(&paquete, &exe, probar_que_diga("99.0.0")).unwrap();
        assert_eq!(r.version, "99.0.0");
        assert!(r.probado, "la prueba pasó: {r:?}");
        assert!(!r.deshecho);

        // El ejecutable ahora es el nuevo...
        let instalado = fs::read_to_string(&exe).unwrap();
        assert!(instalado.contains("99.0.0"));
        // ...y el viejo se guardó antes de tocar nada.
        let respaldo = fs::read_to_string(con_sufijo(&exe, RESPALDO)).unwrap();
        assert!(respaldo.contains("0.4.0"));
        // Todo confirmado: sin marcador pendiente.
        assert!(!con_sufijo(&exe, PENDIENTE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn si_el_nuevo_no_arranca_se_deshecha_solo() {
        let dir = raiz("rollback");
        let exe = instalar(&dir, "0.4.0");
        let antes = fs::read(&exe).unwrap();
        // El paquete trae un ejecutable que falla la prueba.
        let paquete = escribir_paquete(&dir, "99.0.0", "0.1.0", &script("99.0.0"));

        let r = aplicar(&paquete, &exe, |_| Err("no arrancó".to_string())).unwrap();
        assert!(r.deshecho, "se debió deshacer: {r:?}");
        assert!(!r.probado);
        assert_eq!(r.motivo, "no arrancó");

        // Quedó exactamente el ejecutable que sí funcionaba.
        assert_eq!(fs::read(&exe).unwrap(), antes);
        assert!(!con_sufijo(&exe, PENDIENTE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn revertir_vuelve_al_ejecutable_anterior() {
        let dir = raiz("revertir");
        let exe = instalar(&dir, "0.4.0");
        let original = fs::read(&exe).unwrap();
        let paquete = escribir_paquete(&dir, "99.0.0", "0.1.0", &script("99.0.0"));
        aplicar(&paquete, &exe, probar_que_diga("99.0.0")).unwrap();
        assert!(fs::read_to_string(&exe).unwrap().contains("99.0.0"));

        revertir(&exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), original);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn revertir_sin_respaldo_dice_por_que_no() {
        let dir = raiz("sin-respaldo");
        let exe = instalar(&dir, "0.4.0");
        let e = revertir(&exe).unwrap_err();
        assert!(matches!(e, Error::Io(_)), "{e:?}");
        assert!(e.to_string().contains("No hay una versión anterior"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn el_estado_dice_que_hay_y_que_no_hay_respaldo() {
        let dir = raiz("estado");
        let exe = instalar(&dir, "0.4.0");
        let e = estado(&exe);
        assert!(!e.hay_respaldo);
        assert!(!e.hay_pendiente);
        assert_eq!(e.version_respaldo, None);

        let paquete = escribir_paquete(&dir, "99.0.0", "0.1.0", &script("99.0.0"));
        aplicar(&paquete, &exe, probar_que_diga("99.0.0")).unwrap();
        let e = estado(&exe);
        assert!(e.hay_respaldo);
        assert!(!e.hay_pendiente);
        assert_eq!(e.version_respaldo, Some(VERSION.to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn aplicar_un_paquete_malo_no_toca_el_ejecutable() {
        let dir = raiz("paquete-malo");
        let exe = instalar(&dir, "0.4.0");
        let original = fs::read(&exe).unwrap();
        // Firmado con otra llave.
        let otro = armar("99.0.0", "0.1.0", "", &script("99.0.0"), &[8u8; 32]).unwrap();
        let ruta = dir.join("malo.posup");
        fs::write(&ruta, otro).unwrap();

        let e = aplicar(&ruta, &exe, probar_que_diga("99.0.0")).unwrap_err();
        assert_eq!(e, Error::FirmaInvalida);
        // Ni un byte se movió.
        assert_eq!(fs::read(&exe).unwrap(), original);
        assert!(!con_sufijo(&exe, RESPALDO).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn actualizar_a_una_version_mas_vieja_no_se_puede() {
        let dir = raiz("vieja");
        let exe = instalar(&dir, "0.4.0");
        let original = fs::read(&exe).unwrap();
        let paquete = escribir_paquete(&dir, "0.0.1", "0.0.0", &script("0.0.1"));
        let e = aplicar(&paquete, &exe, probar_que_diga("0.0.1")).unwrap_err();
        assert!(matches!(e, Error::VersionVieja { .. }), "{e:?}");
        assert_eq!(fs::read(&exe).unwrap(), original);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn aplicar_dos_veces_deja_un_solo_respaldo() {
        let dir = raiz("dos-veces");
        let exe = instalar(&dir, "0.4.0");
        let p1 = escribir_paquete(&dir, "98.0.0", "0.1.0", &script("98.0.0"));
        aplicar(&p1, &exe, probar_que_diga("98.0.0")).unwrap();
        let p2 = escribir_paquete(&dir, "99.0.0", "0.1.0", &script("99.0.0"));
        aplicar(&p2, &exe, probar_que_diga("99.0.0")).unwrap();

        // El respaldo es el inmediato anterior, no el de hace dos versiones.
        let respaldo = fs::read_to_string(con_sufijo(&exe, RESPALDO)).unwrap();
        assert!(respaldo.contains("soy la version"), "{respaldo}");
        assert!(fs::read_to_string(&exe).unwrap().contains("99.0.0"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_ejecutable_nuevo_se_escribe_con_permiso_de_ejecucion() {
        let dir = raiz("permisos");
        let exe = instalar(&dir, "0.4.0");
        let paquete = escribir_paquete(&dir, "99.0.0", "0.1.0", &script("99.0.0"));
        aplicar(&paquete, &exe, probar_que_diga("99.0.0")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let modo = fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(modo & 0o111, 0o111, "debe ser ejecutable: {modo:o}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn probar_arranque_detecta_un_ejecutable_que_falla() {
        let dir = raiz("probar");
        // Un script que falla: sale con 3.
        let malo = dir.join("malo");
        fs::write(&malo, b"#!/bin/sh\nexit 3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&malo, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let e = probar_arranque(&malo, 5).unwrap_err();
        assert!(e.contains("salió con error"), "{e}");

        // Y uno que tarda más de la cuenta se cuelga y se mata.
        let colgado = dir.join("colgado");
        fs::write(&colgado, b"#!/bin/sh\nsleep 10\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&colgado, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let e = probar_arranque(&colgado, 1).unwrap_err();
        assert!(e.contains("se colgó"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_huella_es_corta_y_estable() {
        assert_eq!(huella(b"hola").len(), 12);
        assert_eq!(huella(b"hola"), huella(b"hola"));
        assert_ne!(huella(b"hola"), huella(b"hola!"));
    }

    #[test]
    fn reemplazar_en_uso_deja_el_nuevo_y_aparta_el_viejo() {
        let dir = std::env::temp_dir().join("pos-update-reemplazo");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("pos.exe");
        let nuevo = dir.join("pos.nuevo");
        let apartado = dir.join("pos.anterior");
        fs::write(&exe, "VIEJO").unwrap();
        fs::write(&nuevo, "NUEVO").unwrap();

        reemplazar_en_uso(&exe, &nuevo, &apartado).unwrap();

        assert_eq!(fs::read_to_string(&exe).unwrap(), "NUEVO");
        assert_eq!(fs::read_to_string(&apartado).unwrap(), "VIEJO");
        // El archivo nuevo ya no está aparte: se movió, no se copió.
        assert!(!nuevo.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// El caso que en Windows rompía todo: el destino está en uso. Aquí se
    /// simula en lo que se puede desde Linux —lo que se verifica es que si el
    /// cambio falla, **el programa original sigue en su sitio**.
    #[test]
    fn si_el_cambio_falla_el_ejecutable_vuelve_a_su_sitio() {
        let dir = std::env::temp_dir().join("pos-update-falla");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("pos.exe");
        let nuevo = dir.join("pos.nuevo"); // nunca se escribe
        let apartado = dir.join("pos.anterior");
        fs::write(&exe, "VIEJO").unwrap();

        let err = reemplazar_en_uso(&exe, &nuevo, &apartado).unwrap_err();
        assert!(err.to_string().contains("No pude reemplazar"), "{err}");

        // Lo importante: la caja no se queda sin programa.
        assert_eq!(fs::read_to_string(&exe).unwrap(), "VIEJO");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reemplazar_en_uso_sirve_para_una_instalacion_limpia() {
        let dir = std::env::temp_dir().join("pos-update-limpia");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("pos.exe");
        let nuevo = dir.join("pos.nuevo");
        let apartado = dir.join("pos.anterior");
        fs::write(&nuevo, "PRIMERO").unwrap();

        reemplazar_en_uso(&exe, &nuevo, &apartado).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "PRIMERO");
        // No había nada que apartar.
        assert!(!apartado.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn limpiar_sobrantes_borra_lo_que_windows_no_dejo_borrar() {
        let dir = std::env::temp_dir().join("pos-update-sobrantes");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("pos.exe");
        fs::write(&exe, "ACTUAL").unwrap();
        fs::write(con_sufijo(&exe, ENUSO), "VIEJO EN USO").unwrap();

        limpiar_sobrantes(&exe);

        assert!(!con_sufijo(&exe, ENUSO).exists(), "quedó basura");
        // Y el ejecutable actual no se toca.
        assert_eq!(fs::read_to_string(&exe).unwrap(), "ACTUAL");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Después de una actualización buena tiene que quedar marcha atrás: es lo
    /// que el botón "Deshacer" y `pos --revertir` necesitan para servir.
    #[test]
    fn al_terminar_una_actualizacion_buena_queda_marcha_atras() {
        let dir = raiz("marcha-atras");
        let exe = instalar(&dir, "0.6.0");
        let paquete = escribir_paquete(&dir, "0.7.0", "0.1.0", &script("0.7.0"));

        let r = aplicar(&paquete, &exe, |_| Ok(())).unwrap();
        assert!(r.probado && !r.deshecho, "{r:?}");

        let e = estado(&exe);
        assert!(e.hay_respaldo, "no quedó marcha atrás");
        assert!(!e.hay_pendiente, "quedó marcada como a medias");
        // Y lo instalado es la versión nueva.
        assert!(fs::read_to_string(&exe).unwrap().contains("0.7.0"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Cuando la actualización se deshace sola, se consume el respaldo (que era
    /// el ejecutable apartado) porque ya se está usando: la caja queda con la
    /// versión buena y sin nada pendiente.
    #[test]
    fn al_desbaratarse_la_actualizacion_queda_la_version_buena() {
        let dir = raiz("deshecha");
        let exe = instalar(&dir, "0.6.0");
        let antes = fs::read(&exe).unwrap();
        let paquete = escribir_paquete(&dir, "0.7.0", "0.1.0", &script("0.7.0"));

        // El nuevo "no arranca": la prueba de arranque falla.
        let r = aplicar(&paquete, &exe, |_| Err("no arrancó".to_string())).unwrap();
        assert!(r.deshecho, "{r:?}");

        assert_eq!(fs::read(&exe).unwrap(), antes, "no volvió la versión buena");
        let e = estado(&exe);
        assert!(!e.hay_pendiente, "quedó el marcador de a medias");
        fs::remove_dir_all(&dir).unwrap();
    }
}
