//! `pos-core` — el corazón del punto de venta.
//!
//! Regla de oro: **aquí no hay I/O**. Ni SQLite, ni impresora, ni red.
//! Todo el dominio se expresa con tipos puros para que las reglas de negocio
//! (impuestos, descuentos, cambios, cortes) se prueben en microsegundos.

pub mod caja;
pub mod ean;
pub mod error;
pub mod money;
pub mod sale;
pub mod tax;
pub mod tiempo;
pub mod users;

pub use caja::{
    validar_movimiento, Arqueo, Cierre, CorteCaja, EstadoCaja, MovimientoCaja, DENOMINACIONES_MXN,
};
pub use ean::{digito_control, ean13_de, ean13_interno, ean13_valido, PREFIJO_INTERNO};
pub use error::{CoreError, Result};
pub use money::{Money, Quantity};
pub use sale::{Payment, PaymentMethod, Sale, SaleLine, SaleStatus};
pub use tax::{tax_added_amount, tax_included_amount, TaxRate};
pub use tiempo::{ahora_segs, civil_desde_dias, FechaHora, OFFSET_CDMX_SEGS};
pub use users::{Permiso, Pin, Role, User, MAX_INTENTOS_PIN};
