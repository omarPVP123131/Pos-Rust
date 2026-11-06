//! Servidor HTTP mínimo para la red local.
//!
//! **Sin tokio, sin axum, sin hyper**: sólo `std::net`. Corre en la misma PC
//! del punto de venta, donde cada mega de RAM cuenta. Un hilo por conexión es
//! más que suficiente para una tienda con dos o tres dispositivos.
//!
//! Contrato: `POST /api` con el JSON de [`pos_api::Peticion`] → JSON de
//! [`pos_api::Respuesta`]. La app móvil usa exactamente los mismos comandos
//! que la interfaz de escritorio.

pub mod http;

pub use http::{servir, servir_en_puerto_libre, ApiCompartida};
