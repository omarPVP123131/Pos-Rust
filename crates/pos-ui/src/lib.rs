//! Interfaz de mostrador (egui).
//!
//! ## Este crate es una *piel*, no el sistema
//!
//! No importa `pos-db`, no sabe que existe SQLite y no contiene ninguna regla
//! de negocio: todo lo que hace es dibujar y llamar a [`pos_api::PosApi`].
//!
//! Por eso **cambiar de motor de interfaz es un cambio cosmético**: reescribir
//! este crate con iced, Slint, Tauri o Flutter no toca la base de datos, ni
//! los impuestos, ni el inventario, ni la caja. Y por eso una app móvil puede
//! consumir los mismos comandos por la LAN (ver `pos-server`).
//!
//! egui dibuja con OpenGL en modo **reactivo**: la CPU descansa hasta que el
//! cajero toca algo. Es lo que permite que esto corra en un Celeron con 1 GB.

pub mod apariencia;
pub mod app;
pub mod fotos;
pub mod iconos;
pub mod theme;

pub use app::{Pantalla, PosApp};

use eframe::egui;
use pos_api::PosApi;

/// Abre la ventana del punto de venta contra cualquier implementación de la
/// API: la local (misma PC) o la remota (otra PC por la LAN).
///
/// En Windows las Celeron viejas a veces no traen OpenGL 3.3 (o traen un
/// driver roto). Si `POS_SOFTWARE_GL=1` (o `LIBGL_ALWAYS_SOFTWARE=1`) se
/// arranca en software (WARP): lento pero abre. Si la ventana no abre, el
/// error queda en `error.log` con el consejo para el técnico.
pub fn run(api: Box<dyn PosApi>, dry_run: bool) -> eframe::Result<()> {
    let software = std::env::var_os("POS_SOFTWARE_GL").is_some()
        || std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_some();
    let opciones = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([1024.0, 600.0])
            .with_title("Punto de Venta")
            .with_app_id("punto-de-venta"),
        vsync: true,
        hardware_acceleration: if software {
            eframe::HardwareAcceleration::Off
        } else {
            eframe::HardwareAcceleration::Preferred
        },
        persist_window: false,
        // Sin `persistence`: menos disco, arranque más rápido.
        ..Default::default()
    };

    let nombre: &str = "punto-de-venta";
    // Nota: en eframe 0.31 `run_native` pide `&str`; en versiones anteriores
    // pedía `String`. Si actualizas, basta con `nombre.to_owned()`.
    let creador: eframe::AppCreator = Box::new(move |cc: &eframe::CreationContext<'_>| {
        // La apariencia se lee una sola vez, aquí: lo que se haya elegido la
        // última vez se ve desde el primer frame, sin un parpadeo de por medio.
        let apariencia = theme::Apariencia::cargar();
        theme::aplicar_con(&cc.egui_ctx, &apariencia);
        Ok(Box::new(
            PosApp::new(api, dry_run).con_apariencia(apariencia),
        ))
    });

    if let Err(e) = eframe::run_native(nombre, opciones, creador) {
        registrar_fallo_ventana(&format!("{e:?}"));
        return Err(e);
    }
    Ok(())
}

/// Deja el error de ventana en `error.log` (mismo lugar que el panic hook),
/// para que el técnico lo encuentre aunque no haya consola.
fn registrar_fallo_ventana(texto: &str) {
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
    let consejo = "Si dice OpenGL/WGL/context: actualiza el driver de video o reintenta con POS_SOFTWARE_GL=1. Sin ventana puedes vender desde el celular con pos --servir.\n";
    let _ = std::fs::write(
        &ruta,
        format!("Fallo al abrir la ventana:\n{texto}\n{consejo}"),
    );
}
