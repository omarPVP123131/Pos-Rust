//! Ejecutable del punto de venta.
//!
//! Tres formas de arrancar, **todas con la misma API**:
//!
//! * modo normal: interfaz de escritorio contra `LocalApi` (misma PC).
//! * `--api http://192.168.1.50:8080`: interfaz de escritorio contra otra PC.
//! * `--servir` / `--headless`: sin interfaz; queda escuchando para la app
//!   móvil (o el script de Python) en la LAN.
//!
//! Que los tres modos existan con el mismo binario es la prueba de que la
//! interfaz es intercambiable: el motor de ventanas no es el sistema.
//!
//! En Windows el .exe es una aplicación de escritorio (`windows_subsystem`):
//! no abre una consola negra detrás del mostrador. Si se pide una bandera de
//! terminal (`--impresoras`, `--servir`, …) se adjunta o se crea una consola.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::io::Write;

use pos_api::{HttpApi, LocalApi, PosApi};
use pos_core::{ahora_segs, FechaHora};
use pos_db::Database;
use pos_update::aplicar;
use pos_update::paquete;

mod guion;

/// Dónde vive la base de datos si nadie dice otra cosa.
///
/// En Linux se deja junto al programa (`pos.db`): es lo que espera quien corre
/// el punto de venta desde una memoria USB o lo instala con el `.deb`, y
/// permite tener la tienda completa en una carpeta.
///
/// En Windows eso **no funciona**: instalado en *Archivos de programa*, el
/// usuario normal no puede escribir ahí, así que la base no se crea (o Windows
/// la guarda en una carpeta escondida y "mágica" que nadie encuentra). Allí la
/// base va en la carpeta de datos del usuario, que sí es suya. Los respaldos
/// salen al lado de la base, así que también quedan en un lugar que se puede
/// copiar a la memoria USB.
fn ruta_base_por_omision() -> String {
    if cfg!(windows) {
        let carpeta = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(std::path::PathBuf::from);
        if let Some(base) = carpeta {
            let dir = base.join("PuntoDeVenta");
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir.join("pos.db").to_string_lossy().to_string();
            }
        }
    }
    "pos.db".to_string()
}

fn main() {
    instalar_registro_de_errores();
    let args: Vec<String> = std::env::args().collect();

    let consola = pide_consola(&args);
    #[cfg(windows)]
    if consola {
        consola::preparar();
    } else {
        // Sin consola, println!/eprintln! abortan el proceso (el .exe es
        // windows_subsystem). Al entrar, la bitácora o el respaldo imprimen
        // y Windows mata la ventana. Se mandan a NUL.
        consola::silenciar();
    }
    let _ = consola;

    // Windows no deja borrar un `.exe` mientras corre: al actualizar, el
    // ejecutable viejo se aparta y se limpia aquí, en el siguiente arranque,
    // cuando ya nadie lo está usando. Cuesta una llamada al sistema.
    if let Ok(exe) = std::env::current_exe() {
        pos_update::aplicar::limpiar_sobrantes(&exe);
    }

    let mut db_path = std::env::var("POS_DB").unwrap_or_else(|_| ruta_base_por_omision());
    let mut api_url: Option<String> = None;
    let mut servir = false;
    let mut puerto = "8080".to_string();
    let mut dry_run = false;
    let mut respaldar_a: Option<String> = None;
    let mut restaurar_de: Option<String> = None;
    let mut checar = false;
    let mut actualizar_de: Option<String> = None;
    let mut revertir = false;
    let mut listar_impresoras = false;
    let mut probar_impresora: Option<String> = None;
    let mut cajon = false;
    let mut probar_login = false;
    let mut guion_ruta: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                if let Some(v) = args.get(i + 1) {
                    db_path = v.clone();
                    i += 1;
                }
            }
            "--api" => {
                if let Some(v) = args.get(i + 1) {
                    api_url = Some(v.trim_end_matches('/').to_string());
                    i += 1;
                }
            }
            "--servir" | "--headless" | "--sin-ventana" => servir = true,
            "--puerto" => {
                if let Some(v) = args.get(i + 1) {
                    puerto = v.clone();
                    i += 1;
                }
            }
            "--dry-run" => dry_run = true,
            "--respaldar" => {
                // Con o sin carpeta: si no la mandan, usa la configurada.
                respaldar_a = Some(args.get(i + 1).cloned().unwrap_or_default());
                if respaldar_a.as_deref().is_some_and(|v| !v.is_empty()) {
                    i += 1;
                }
            }
            "--restaurar" => match args.get(i + 1) {
                Some(v) => {
                    restaurar_de = Some(v.clone());
                    i += 1;
                }
                None => {
                    eprintln!("Falta la ruta del respaldo: pos --restaurar RUTA");
                    std::process::exit(2);
                }
            },
            "--checar" => checar = true,
            "--version" => {
                println!("Punto de venta {}", pos_update::VERSION);
                return;
            }
            "--revertir" => revertir = true,
            "--actualizar" => match args.get(i + 1) {
                Some(v) => {
                    actualizar_de = Some(v.clone());
                    i += 1;
                }
                None => {
                    eprintln!("Falta la ruta del paquete: pos --actualizar RUTA");
                    std::process::exit(2);
                }
            },
            "--impresoras" => listar_impresoras = true,
            "--probar-login" => probar_login = true,
            "--probar-impresion" => {
                // Con o sin ruta: sin ruta usa la impresora configurada.
                probar_impresora = Some(args.get(i + 1).cloned().unwrap_or_default());
                if probar_impresora.as_deref().is_some_and(|v| !v.is_empty()) {
                    i += 1;
                }
            }
            "--cajon" => cajon = true,
            "--guion" => match args.get(i + 1) {
                Some(v) => {
                    guion_ruta = Some(v.clone());
                    i += 1;
                }
                None => {
                    eprintln!("Falta la ruta del guion: pos --guion RUTA.json");
                    std::process::exit(2);
                }
            },
            "--ayuda" | "-h" => {
                ayuda();
                return;
            }
            otro => {
                eprintln!("No conozco la bandera {otro}\n");
                ayuda();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    // --- tareas de un solo golpe: para cuando la PC se muere ----------------
    if respaldar_a.is_some() || restaurar_de.is_some() {
        std::process::exit(tarea_unica(&db_path, respaldar_a, restaurar_de));
    }

    // --- impresora: lo primero que se prueba al instalar --------------------
    if listar_impresoras {
        std::process::exit(listar_impresoras_cmd());
    }
    if let Some(ruta) = probar_impresora {
        std::process::exit(probar_impresion(&db_path, &ruta, cajon));
    }

    // --- actualizaciones desde la terminal (sin interfaz) -------------------
    if checar {
        std::process::exit(checar_instalacion(&db_path));
    }
    if probar_login {
        std::process::exit(probar_login_cmd(&db_path));
    }
    if let Some(ruta) = actualizar_de {
        std::process::exit(actualizar(&ruta));
    }
    if revertir {
        std::process::exit(revertir_instalacion());
    }

    // --- guiones headless: los mismos comandos, sin abrir ventana -----------
    if let Some(ruta) = guion_ruta {
        let mut db = match Database::open(&db_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("No se pudo abrir la base de datos '{db_path}': {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = sembrar(&mut db) {
            eprintln!("No se pudo preparar la base: {e}");
            std::process::exit(1);
        }
        let api: Box<dyn PosApi> = Box::new(LocalApi::new(db));
        std::process::exit(guion::ejecutar(api, &ruta, &db_path));
    }

    // --- modo servidor (cabecera de red para la app móvil) ------------------
    if servir {
        let mut db = match Database::open(&db_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("No se pudo abrir la base de datos '{db_path}': {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = sembrar(&mut db) {
            eprintln!("No se pudo preparar la base: {e}");
            std::process::exit(1);
        }
        // El respaldo automático corre aunque la interfaz nunca se abra.
        arrancar_respaldos(&db_path);
        let api: std::sync::Arc<std::sync::Mutex<Box<dyn PosApi>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Box::new(LocalApi::new(db))));
        println!("Sirviendo el punto de venta en el puerto {puerto}.");
        for ip in direcciones_lan() {
            println!("  App móvil: apunta a http://{ip}:{puerto}");
        }
        if let Err(e) = pos_server::servir(api, &format!("0.0.0.0:{puerto}")) {
            eprintln!("El servidor se cayó: {e}");
            std::process::exit(1);
        }
        return;
    }

    // --- modo escritorio ----------------------------------------------------
    let api: Box<dyn PosApi> = match api_url.clone() {
        // Conectado a otra PC: la interfaz ni se entera de que hay red de
        // por medio, porque habla el mismo trait.
        Some(url) => {
            println!("Usando el punto de venta remoto en {url}");
            let mut http = HttpApi::new(&url);
            match http.usuarios_login() {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("No se pudo conectar a {url}: {e}");
                    std::process::exit(1);
                }
            }
            Box::new(http)
        }
        None => {
            let mut db = match Database::open(&db_path) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("No se pudo abrir la base de datos '{db_path}': {e}");
                    std::process::exit(1);
                }
            };
            if let Err(e) = sembrar(&mut db) {
                eprintln!("No se pudo preparar la base: {e}");
                std::process::exit(1);
            }
            // Segundo plano: respalda solo, sin congelar la venta.
            arrancar_respaldos(&db_path);
            Box::new(LocalApi::new(db))
        }
    };

    if let Err(e) = pos_ui::run(api, dry_run) {
        let texto = format!("{e:?}");
        let minus = texto.to_lowercase();
        if minus.contains("gl")
            || minus.contains("wgl")
            || minus.contains("context")
            || minus.contains("gpu")
            || minus.contains("opengl")
            || minus.contains("eventloop")
            || minus.contains("winit")
        {
            eprintln!("No se pudo abrir la ventana ({e}).");
            eprintln!("En Windows con video viejo: actualiza el driver o reintenta con software:");
            eprintln!("  set POS_SOFTWARE_GL=1 && pos.exe");
            eprintln!("Sin ventana puedes seguir vendiendo desde el celular: pos.exe --servir");
            eprintln!("Detalle guardado en %LOCALAPPDATA%\\PuntoDeVenta\\error.log");
        } else {
            eprintln!("La interfaz terminó con error: {e}");
        }
        std::process::exit(1);
    }
}

/// Arranca el hilo de respaldos automáticos.
///
/// Va en un hilo aparte **a propósito**: un `VACUUM INTO` sobre una base de
/// varios cientos de megas tarda sus segundos, y la caja no se puede congelar
/// a media venta. El hilo tiene su propia conexión (SQLite en WAL lo permite)
/// y sólo escribe en la carpeta de respaldos.
fn arrancar_respaldos(db_path: &str) {
    let db_path = db_path.to_string();
    std::thread::spawn(move || {
        // Primera espera corta: que el programa termine de arrancar y migrar.
        std::thread::sleep(std::time::Duration::from_secs(20));
        let api = match Database::open(&db_path) {
            Ok(db) => LocalApi::new(db),
            Err(e) => {
                eprintln!("[respaldo] No pude abrir la base para respaldar: {e}");
                return;
            }
        };
        if let Some(m) = api.tick_respaldo() {
            println!("[respaldo] {m}");
        }
        loop {
            // Cada 15 minutos revisa si ya toca. Revisar es gratis: sólo
            // mira la fecha del último respaldo.
            std::thread::sleep(std::time::Duration::from_secs(15 * 60));
            if let Some(m) = api.tick_respaldo() {
                println!("[respaldo] {m}");
            }
        }
    });
}

/// `pos --respaldar` y `pos --restaurar`: la salida de emergencia.
fn tarea_unica(db_path: &str, respaldar: Option<String>, restaurar: Option<String>) -> i32 {
    if let Some(carpeta) = respaldar {
        let db = match Database::open(db_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("No se pudo abrir la base: {e}");
                return 1;
            }
        };
        let api = LocalApi::new(db);
        return match api.respaldar(true) {
            Ok(m) => {
                println!("{m}");
                if !carpeta.is_empty() {
                    println!("(en {carpeta})");
                }
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }

    if let Some(origen) = restaurar {
        return match restaurar_base(db_path, &origen) {
            Ok(m) => {
                println!("{m}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }
    0
}

/// Reemplaza la base por un respaldo, **verificándolo antes** y dejando una
/// copia de lo que había, por si alguien se equivoca de archivo.
fn restaurar_base(db_path: &str, origen: &str) -> Result<String, String> {
    let ruta = std::path::Path::new(origen);
    if !ruta.exists() {
        return Err(format!("No existe el respaldo: {origen}"));
    }
    // 1. Verificar que el respaldo sirve. Nunca se restauran datos dudosos.
    if !pos_db::respaldo::verificar(ruta).map_err(|e| e.to_string())? {
        return Err(format!(
            "El respaldo está dañado: {origen}. No se tocó tu base de datos."
        ));
    }
    // 2. Comprobar que de verdad es un punto de venta.
    let copia = Database::open(ruta).map_err(|e| e.to_string())?;
    let tiene_ventas = copia
        .conn()
        .query_row("SELECT COUNT(*) FROM sales", [], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?;
    drop(copia);

    // 3. Respaldo de seguridad de lo que había, por si acaso.
    if std::path::Path::new(db_path).exists() {
        let marca = FechaHora::cdmx(ahora_segs()).compacto();
        let seguridad = format!("{db_path}.antes-de-restaurar-{marca}");
        std::fs::copy(db_path, &seguridad).map_err(|e| e.to_string())?;
        // Se van los archivos WAL viejos: mezclar WAL de otra base corrompe.
        for extra in [format!("{db_path}-wal"), format!("{db_path}-shm")] {
            if std::path::Path::new(&extra).exists() {
                let _ = std::fs::remove_file(&extra);
            }
        }
        std::fs::copy(origen, db_path).map_err(|e| e.to_string())?;
        return Ok(format!(
            "Restaurado desde {origen} ({tiene_ventas} ventas en el respaldo).\n\
             Lo que había quedó guardado en: {seguridad}"
        ));
    }

    std::fs::copy(origen, db_path).map_err(|e| e.to_string())?;
    Ok(format!(
        "Base nueva creada desde {origen} ({tiene_ventas} ventas)."
    ))
}

/// Lo que corre el programa **nuevo** para demostrar que sirve, antes de que
/// el actualizador dé el cambio por bueno. Sale con 0 si todo está bien.
/// Si el programa se cae, el cajero no ve la terminal: se deja escrito
/// junto a la base (Windows: %LOCALAPPDATA%\PuntoDeVenta\error.log).
fn instalar_registro_de_errores() {
    std::panic::set_hook(Box::new(|info| {
        let donde = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(std::path::PathBuf::from)
            .map(|p| p.join("PuntoDeVenta"))
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|e| e.parent().map(|p| p.to_path_buf()))
            })
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let _ = std::fs::create_dir_all(&donde);
        let ruta = donde.join("error.log");
        let texto = format!("{info}\n");
        let _ = std::fs::write(&ruta, &texto);
        // println!/eprintln! abortan si no hay consola. writeln! no.
        let _ = writeln!(
            std::io::stderr(),
            "El programa se detuvo: {info}\nQuedó escrito en {}",
            ruta.display()
        );
    }));
}

/// Prueba de humo sin ventana: abre la base, entra y busca. Es lo que se
/// rompía en Windows al pasar del login a la venta.
fn probar_login_cmd(db_path: &str) -> i32 {
    let mut db = match Database::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("No se pudo abrir la base '{db_path}': {e}");
            return 1;
        }
    };
    if let Err(e) = sembrar(&mut db) {
        eprintln!("No se pudo preparar la base: {e}");
        return 1;
    }
    let mut api = LocalApi::new(db);
    match api.login("admin", "1234") {
        Ok(sesion) => {
            println!(
                "ok login {} ({})",
                sesion.usuario.display_name, sesion.usuario.role
            );
            let token = sesion.token;
            match api.buscar(&token, "", 48) {
                Ok(lista) => println!("ok buscar {} producto(s)", lista.len()),
                Err(e) => {
                    eprintln!("Falló la búsqueda después de entrar: {e}");
                    return 1;
                }
            }
            // Lo mismo que hace `entrar()` en la interfaz, que es lo que
            // crasheaba al pasar del PIN a la venta.
            for (nombre, comando) in [
                ("caja", pos_api::Comando::EstadoCaja),
                (
                    "clientes",
                    pos_api::Comando::Clientes {
                        termino: String::new(),
                    },
                ),
                ("categorias", pos_api::Comando::Categorias),
                ("alertas", pos_api::Comando::ProductosBajoMinimo),
            ] {
                match api.ejecutar(pos_api::Peticion {
                    token: Some(token.clone()),
                    comando,
                }) {
                    pos_api::Respuesta::Ok(_) => println!("ok {nombre}"),
                    pos_api::Respuesta::Error(e) => {
                        eprintln!("Falló {nombre} después de entrar: {e}");
                        return 1;
                    }
                }
            }
            0
        }
        Err(e) => {
            eprintln!("No se pudo entrar: {e}");
            1
        }
    }
}

fn checar_instalacion(db_path: &str) -> i32 {
    println!("ok {}", pos_update::VERSION);
    // Si hay base de datos, que también la pueda abrir: un binario nuevo que
    // no entiende su propia base es peor que no actualizar.
    if !std::path::Path::new(db_path).exists() {
        return 0;
    }
    match Database::open(db_path) {
        Ok(db) => match db
            .conn()
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
        {
            Ok(respuesta) if respuesta == "ok" => 0,
            Ok(respuesta) => {
                eprintln!("La base de datos no está íntegra: {respuesta}");
                1
            }
            Err(e) => {
                eprintln!("No pude abrir la base de datos: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("No pude abrir la base de datos: {e}");
            1
        }
    }
}

/// Actualiza desde un paquete firmado, diciendo qué va a pasar.
fn actualizar(ruta: &str) -> i32 {
    let bytes = match std::fs::read(ruta) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("No pude leer {ruta}: {e}");
            return 1;
        }
    };
    match paquete::revisar(&bytes, pos_update::VERSION) {
        Ok(p) => {
            println!(
                "Paquete: {} (desde la {})",
                p.version(),
                p.manifiesto.min_version
            );
            if !p.manifiesto.notas.is_empty() {
                println!("  {}", p.manifiesto.notas);
            }
            println!(
                "  {} bytes · huella {} · firma válida",
                p.manifiesto.tamanio,
                aplicar::huella(&p.payload)
            );
        }
        Err(e) => {
            eprintln!("No se puede instalar: {e}");
            return 1;
        }
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("No sé dónde está instalado el programa: {e}");
            return 1;
        }
    };
    // Veinte segundos para arrancar: en un Celeron con disco lento alcanza.
    let probar = |exe: &std::path::Path| aplicar::probar_arranque(exe, 20);
    match aplicar::aplicar(std::path::Path::new(ruta), &exe, probar) {
        Ok(r) if r.deshecho => {
            eprintln!(
                "No se pudo dejar la {}: {}. Se quedó la versión anterior.",
                r.version, r.motivo
            );
            1
        }
        Ok(r) => {
            println!("Listo: quedó instalada la {}.", r.version);
            println!(
                "La versión anterior quedó en {}.anterior por si algo sale mal.",
                exe.display()
            );
            0
        }
        Err(e) => {
            eprintln!("No se pudo actualizar: {e}");
            1
        }
    }
}

/// Vuelve al ejecutable anterior.
fn revertir_instalacion() -> i32 {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("No sé dónde está instalado el programa: {e}");
            return 1;
        }
    };
    match aplicar::revertir(&exe) {
        Ok(()) => {
            println!("Se volvió a la versión anterior. Abre el programa.");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Lista lo que haya conectado: es lo primero que se corre al instalar.
fn listar_impresoras_cmd() -> i32 {
    let lista = pos_hw::enumerar();
    if lista.is_empty() {
        println!("No encontre ninguna impresora.");
        println!("Revisa que este conectada y encendida, y vuelve a correr: pos --impresoras");
        return 1;
    }
    println!("Impresoras encontradas: {}", lista.len());
    for i in &lista {
        let marca = if i.es_impresora {
            "impresora"
        } else {
            "puerto (no confirmado)"
        };
        println!("  {}  [{}]", i.etiqueta, marca);
        println!("      ruta: {}", i.ruta);
        println!(
            "      perfil sugerido: {} a {} baudios",
            i.perfil.nombre, i.perfil.baudios
        );
        println!("      {}", i.nota);
    }
    let confirmadas = lista.iter().filter(|i| i.es_impresora).count();
    if confirmadas == 1 {
        if let Some(m) = pos_hw::mejor(&lista) {
            println!("\nRecomendada: {}", m.ruta);
            println!("Para probarla:  pos --probar-impresion {}", m.ruta);
        }
    } else if confirmadas > 1 {
        println!("\nHay {confirmadas} impresoras confirmadas. Elige una y pruébala:");
        println!("  pos --probar-impresion RUTA");
    } else {
        println!("\nNinguna está confirmada como impresora (clase USB 07).");
        println!("No adivino el modelo. Elige una ruta de la lista y pruébala:");
        println!("  pos --probar-impresion RUTA");
    }
    0
}

/// Manda un ticket de prueba. Sirve para instalar: si sale bien, ya quedó.
fn probar_impresion(db_path: &str, ruta: &str, abrir_cajon: bool) -> i32 {
    let db = match Database::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("No se pudo abrir la base: {e}");
            return 1;
        }
    };
    use pos_db::settings::Settings;
    let conn = db.conn();
    let tienda = Settings::get_or(conn, "store_name", "Mi Tienda");
    // Sin ruta se usa la que ya está configurada.
    let ruta = if ruta.trim().is_empty() {
        Settings::get_or(conn, "printer_path", "")
    } else {
        ruta.trim().to_string()
    };
    let perfil = pos_hw::Perfil::por_nombre(&Settings::get_or(
        conn,
        "printer_profile",
        &pos_hw::Perfil::default().nombre,
    ))
    .unwrap_or_default()
    .con_columnas(Settings::as_i64(conn, "ticket_width", 48) as usize)
    .con_baudios(Settings::as_i64(conn, "printer_baud", 9600) as u32);

    if ruta.trim().is_empty() {
        eprintln!("No hay impresora configurada y no me dijiste ninguna ruta.");
        eprintln!("Corre primero: pos --impresoras");
        return 2;
    }

    let transporte = pos_hw::Transporte::desde_texto(&ruta, perfil.baudios);
    let bytes = pos_hw::ticket_de_prueba(&perfil, &tienda, &ruta, abrir_cajon);
    println!("Mandando {} bytes a {}", bytes.len(), transporte.etiqueta());
    match transporte.imprimir(&perfil, &bytes) {
        Ok(()) => {
            if let Err(e) = Settings::set(conn, "printer_path", &ruta) {
                eprintln!("Imprimió, pero no pude guardar la ruta: {e}");
            } else {
                let _ = Settings::set(conn, "printer_profile", &perfil.nombre);
                let _ = Settings::set(conn, "printer_baud", &perfil.baudios.to_string());
                println!("Quedó guardada: {ruta}");
            }
            println!("Listo. En el papel mira cuatro cosas:");
            println!("  1. Acentos (á é í ó ú ñ)");
            println!("  2. La regla de números: si se corta, el ancho está mal");
            println!("  3. El QR: si sale basura, usa el perfil «Vieja sin QR»");
            println!("  4. El corte de la cuchilla");
            println!("Si salió cortado o con símbolos raros: pos --probar-impresion con otro perfil o velocidad.");
            0
        }
        Err(e) => {
            eprintln!("No se pudo imprimir: {e}");
            1
        }
    }
}

fn ayuda() {
    println!(
        "Punto de venta\n\n\
         Uso:\n  \
         pos                          interfaz de escritorio (base local)\n  \
         pos --api http://IP:PUERTO   interfaz contra otra PC de la LAN\n  \
         pos --servir / --headless    sin ventana: API para el celular (LAN)\n\n\
         Banderas:\n  \
         --db RUTA                archivo de la base de datos (pos.db; en\n  \
         Windows, %LOCALAPPDATA%\\PuntoDeVenta\\pos.db)\n  \
         --puerto N               puerto del modo --servir/--headless (8080)\n  \
         --dry-run                no imprime: deja el ticket en vista previa\n  \
         --respaldar [CARPETA]    hace un respaldo y sale (para una tarea programada)\n  \
         --restaurar RUTA         recupera la base desde un respaldo verificado\n  \
         --impresoras             lista las impresoras conectadas y dice que son\n  \
         --probar-impresion [RUTA] manda un ticket de prueba a la impresora\n  \
         --cajon                  con --probar-impresion, tambien abre el cajon\n  \
         --actualizar PAQUETE     instala una actualizacion firmada (.posup)\n  \
         --revertir               vuelve al programa anterior\n  \
         --checar                 dice si el programa arranca (lo usa el actualizador)\n  \
         --guion RUTA.json        ejecuta comandos sin ventana y reporta el resultado\n  \
         --version                la version instalada\n  \
         --ayuda                  esta ayuda\n\n\
         Variables:\n  \
         POS_DB         equivale a --db\n"
    );
}

/// Primer arranque: deja la base lista para usarse.
fn sembrar(db: &mut Database) -> Result<(), String> {
    if let Err(e) = pos_db::users::Users::asegurar_admin(db.conn_mut(), "1234") {
        return Err(e.to_string());
    }
    if let Ok(true) = db
        .conn()
        .query_row("SELECT COUNT(*) = 0 FROM products", [], |r| r.get(0))
        .map_err(|e| e.to_string())
    {
        sembrar_demo(db)?;
    }
    Ok(())
}

/// Catálogo de arranque para poder probar el sistema el primer día.
fn sembrar_demo(db: &mut Database) -> Result<(), String> {
    let demo = [
        ("7501", "Coca-Cola 600 ml", 1800i64, 1200i64, 100_000i64),
        ("7502", "Pan blanco Bimbo", 1850, 1300, 30_000),
        ("7503", "Leche Lala 1 L", 2700, 2100, 40_000),
        ("7504", "Huevo blanco 12 pzas", 4600, 3800, 25_000),
        ("7505", "Arroz Morelos 1 kg", 2350, 1800, 35_000),
        ("7506", "Frijol negro 900 g", 2900, 2200, 20_000),
        ("7507", "Aceite 1-2-3 1 L", 4200, 3400, 15_000),
        ("7508", "Azúcar estándar 1 kg", 2600, 1900, 30_000),
        ("7509", "Café soluble 100 g", 8900, 6500, 12_000),
        ("7510", "Sal de mesa 1 kg", 1250, 800, 40_000),
        ("7511", "Jabón de tocador", 1750, 1100, 24_000),
        ("7512", "Papel higiénico 4 rollos", 5600, 4200, 18_000),
        ("7513", "Cerveza lata 355 ml", 2400, 1700, 60_000),
        ("7514", "Sabritas original 45 g", 2100, 1500, 48_000),
        ("7515", "Galletas Marías 200 g", 1900, 1350, 30_000),
        ("7516", "Agua purificada 1.5 L", 1500, 900, 72_000),
    ];
    for (sku, nombre, precio, costo, stock) in demo {
        pos_db::products::Products::insert(
            db.conn_mut(),
            &pos_db::products::NewProduct {
                sku: sku.into(),
                name: nombre.into(),
                price_cents: precio,
                cost_cents: costo,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: stock,
                min_stock_millis: 5_000,
                track_inventory: true,
                barcodes: vec![format!("{sku}00000")],
            },
        )
        .map_err(|e| e.to_string())?;
    }
    if pos_db::users::Users::by_username(db.conn(), "ana")
        .ok()
        .flatten()
        .is_none()
    {
        let _ = pos_db::users::Users::crear(
            db.conn_mut(),
            "ana",
            "Ana",
            "4321",
            pos_core::Role::Cajero,
        );
    }
    Ok(())
}

fn direcciones_lan() -> Vec<String> {
    pos_hw::direcciones_lan()
}

/// ¿Esta invocación imprime a la terminal? Si sí, en Windows hay que
/// adjuntar (o crear) una consola: el .exe no trae una por omisión.
fn pide_consola(args: &[String]) -> bool {
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => {}
            "--db" | "--api" | "--puerto" => i += 1,
            otro if otro.starts_with('-') => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Consola a demanda en Windows: el mostrador no debe abrir una ventana negra.
#[cfg(windows)]
mod consola {
    use std::io::Write;

    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn AllocConsole() -> i32;
        fn GetConsoleWindow() -> *mut core::ffi::c_void;
        fn GetStdHandle(id: u32) -> *mut core::ffi::c_void;
        fn GetConsoleMode(h: *mut core::ffi::c_void, modo: *mut u32) -> i32;
        fn GetFileType(h: *mut core::ffi::c_void) -> u32;
        fn SetStdHandle(id: u32, h: *mut core::ffi::c_void) -> i32;
        fn CreateFileW(
            nombre: *const u16,
            acceso: u32,
            compartir: u32,
            seguridad: *mut core::ffi::c_void,
            disposicion: u32,
            flags: u32,
            plantilla: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
    }

    const STD_INPUT: u32 = 0xFFFF_FFF6; // -10
    const STD_OUTPUT: u32 = 0xFFFF_FFF5; // -11
    const STD_ERROR: u32 = 0xFFFF_FFF4; // -12
    const FILE_TYPE_UNKNOWN: u32 = 0;
    const FILE_TYPE_CHAR: u32 = 2;

    /// ¿stdout ya sirve? Cubre tres casos:
    /// * debug (.exe con consola): lanzado desde PowerShell/cmd ya trae una.
    /// * release con salida redirigida (`pos.exe --checar > log.txt`, cargo
    ///   capturando la salida): es tubería/archivo, println! ya funciona.
    ///
    ///   Sólo cuando no hay nada (GUI sin consola) hay que adjuntar/crear.
    fn salida_sirve() -> bool {
        unsafe {
            let h = GetStdHandle(STD_OUTPUT);
            if h.is_null() || h == (-1isize) as *mut core::ffi::c_void {
                return false;
            }
            let tipo = GetFileType(h);
            if tipo == FILE_TYPE_UNKNOWN {
                return false;
            }
            if tipo == FILE_TYPE_CHAR {
                // Es un CON: si además es consola de verdad, listo. Si es
                // NUL u otro char sin modo consola, igual escribe sin abortar.
                let mut modo: u32 = 0;
                if GetConsoleMode(h, &mut modo) != 0 {
                    return true;
                }
                // CONOUT$/NUL aceptan escritura: no hace falta reconectar.
                return true;
            }
            // Tubería o archivo: cargo, `> log.txt`, `| Select-Object`.
            true
        }
    }

    fn hay_ventana_consola() -> bool {
        unsafe { !GetConsoleWindow().is_null() }
    }

    /// Adjunta la consola del padre (cmd / PowerShell). Si no hay, crea una.
    /// Hay que hacerlo **antes** del primer `println!`: Rust toma stdout una vez.
    pub fn preparar() {
        // El .exe de debug ya trae consola y la tubería de cargo ya sirve:
        // reconectar a CONOUT$ mandaba la salida a la ventana y `cargo run`
        // se quedaba mudo (y `--checar` parecía no imprimir nada).
        if salida_sirve() && hay_ventana_consola() {
            return;
        }
        if salida_sirve() {
            // Salida redirigida a archivo/tubería sin ventana: se respeta el
            // destino que eligió quien llamó, no se abre una ventana nueva.
            return;
        }
        const ATTACH_PARENT: u32 = 0xFFFF_FFFF;
        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        const OPEN_EXISTING: u32 = 3;

        unsafe {
            if AttachConsole(ATTACH_PARENT) == 0 {
                let _ = AllocConsole();
            }
            let conin: Vec<u16> = "CONIN$\0".encode_utf16().collect();
            let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
            let entrada = CreateFileW(
                conin.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                core::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                core::ptr::null_mut(),
            );
            let salida = CreateFileW(
                conout.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                core::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                core::ptr::null_mut(),
            );
            let _ = SetStdHandle(STD_INPUT, entrada);
            let _ = SetStdHandle(STD_OUTPUT, salida);
            let _ = SetStdHandle(STD_ERROR, salida);
            // Rust toma stdin/stdout al arrancar, cuando todavía no hay
            // consola. `freopen` sobre el FILE del CRT (UCRT) vuelve a
            // conectar println! / eprintln!.
            reconectar_crt(b"CONIN$\0", b"CONOUT$\0");
        }
        // Si CONOUT$ no admite escritura (Wine, sesión sin consola),
        // println! aborta. Una escritura vacía a veces "pasa"; hay que
        // mandar un salto de línea, que es lo que hace println!.
        if writeln!(std::io::stdout()).is_err() || writeln!(std::io::stderr()).is_err() {
            silenciar();
        }
    }

    /// Mandar stdout/stderr a NUL para que un `println!` no aborte.
    /// Si ya hay destino válido (consola del debug o tubería de cargo), no
    /// se toca: silenciar a ciegas es lo que dejaba `--checar` mudo.
    pub fn silenciar() {
        if salida_sirve() {
            return;
        }
        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        const OPEN_EXISTING: u32 = 3;
        unsafe {
            let nul: Vec<u16> = "NUL\0".encode_utf16().collect();
            let h = CreateFileW(
                nul.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                core::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                core::ptr::null_mut(),
            );
            let invalido = (-1isize) as *mut core::ffi::c_void;
            if !h.is_null() && h != invalido {
                let _ = SetStdHandle(STD_OUTPUT, h);
                let _ = SetStdHandle(STD_ERROR, h);
            }
            reconectar_crt(b"NUL\0", b"NUL\0");
        }
    }

    extern "C" {
        fn freopen(
            filename: *const i8,
            mode: *const i8,
            file: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
        fn __acrt_iob_func(ix: u32) -> *mut core::ffi::c_void;
    }

    unsafe fn reconectar_crt(entrada: &[u8], salida: &[u8]) {
        let w = c"w".as_ptr();
        let r = c"r".as_ptr();
        let _ = freopen(entrada.as_ptr() as *const i8, r, __acrt_iob_func(0));
        let _ = freopen(salida.as_ptr() as *const i8, w, __acrt_iob_func(1));
        let _ = freopen(salida.as_ptr() as *const i8, w, __acrt_iob_func(2));
    }
}

#[cfg(test)]
mod impresora_cli {
    use super::*;
    use pos_db::settings::Settings;

    #[test]
    fn al_imprimir_bien_se_guarda_la_ruta() {
        let dir = std::env::temp_dir().join("pos-app-probar-imp");
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("tienda.db");
        let _ = std::fs::remove_file(&db_path);
        let papel = dir.join("ticket.bin");
        std::fs::write(&papel, b"").unwrap();

        let code = probar_impresion(db_path.to_str().unwrap(), papel.to_str().unwrap(), false);
        assert_eq!(code, 0, "el archivo de prueba debe imprimir");

        let db = Database::open(db_path.to_str().unwrap()).unwrap();
        assert_eq!(
            Settings::get_or(db.conn(), "printer_path", ""),
            papel.to_str().unwrap(),
            "si el ticket salió, esa es la impresora de la tienda"
        );
    }

    #[test]
    fn una_ruta_que_no_existe_no_se_guarda() {
        let dir = std::env::temp_dir().join("pos-app-probar-imp-falla");
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("tienda.db");
        let _ = std::fs::remove_file(&db_path);

        let code = probar_impresion(db_path.to_str().unwrap(), "/dev/usb/lpQueNoExiste", false);
        assert_ne!(code, 0);

        let db = Database::open(db_path.to_str().unwrap()).unwrap();
        assert_eq!(
            Settings::get_or(db.conn(), "printer_path", ""),
            "",
            "un fallo no debe dejar una impresora inventada"
        );
    }

    #[test]
    fn el_mostrador_no_pide_consola_y_las_banderas_si() {
        assert!(!pide_consola(&["pos".into()]));
        assert!(!pide_consola(&["pos".into(), "--dry-run".into()]));
        assert!(!pide_consola(&[
            "pos".into(),
            "--db".into(),
            "tienda.db".into()
        ]));
        assert!(pide_consola(&["pos".into(), "--impresoras".into()]));
        assert!(pide_consola(&["pos".into(), "--servir".into()]));
        assert!(pide_consola(&["pos".into(), "--headless".into()]));
        assert!(pide_consola(&["pos".into(), "--version".into()]));
        assert!(pide_consola(&["pos".into(), "--ayuda".into()]));
    }
}
