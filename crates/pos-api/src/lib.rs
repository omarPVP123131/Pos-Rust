//! `pos-api` — la única puerta de entrada al punto de venta.
//!
//! ## La regla de oro del proyecto
//!
//! **Ninguna regla de negocio vive en la interfaz.** La interfaz (egui hoy,
//! iced/Slint/Tauri/Flutter mañana) sólo dibuja y llama a [`PosApi`]. La app
//! móvil en la LAN llama exactamente a lo mismo por HTTP.
//!
//! ```text
//!   ┌───────────────┐   ┌────────────────┐   ┌──────────────┐
//!   │  UI escritorio│   │  App móvil LAN │   │  CLI / tests │
//!   └───────┬───────┘   └────────┬───────┘   └──────┬───────┘
//!           │                    │                  │
//!           └────────────────────┴──────────────────┘
//!                              │
//!                    trait PosApi  (un solo contrato)
//!                              │
//!              ┌───────────────┴────────────────┐
//!              │                                │
//!       LocalApi (en proceso)          HttpApi (JSON/TCP)
//!              │                                │
//!              ▼                                ▼
//!        pos-db + pos-core              pos-server → pos-db + pos-core
//! ```
//!
//! Cambiar de motor de interfaz = reescribir el dibujo, **sin tocar este
//! crate ni la base de datos**. Añadir un cliente nuevo = implementar una
//! llamada HTTP con los mismos comandos.

pub mod api;
pub mod dto;
pub mod fiscal;
pub mod http;
pub mod importar;
pub mod local;

pub use api::{Comando, Cuerpo, Peticion, PosApi, Respuesta};
pub use dto::*;
pub use http::HttpApi;
pub use importar::{exportar, interpretar, ErrorRenglon, ResultadoImporte};
pub use local::LocalApi;

/// Denominaciones de efectivo que entiende el arqueo (centavos, etiqueta).
/// Reexportadas para que cualquier interfaz arme la misma pantalla de corte.
pub use pos_core::DENOMINACIONES_MXN;
