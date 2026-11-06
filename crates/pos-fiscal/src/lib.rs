//! Facturación electrónica mexicana (CFDI 4.0).
//!
//! Dos piezas separadas a propósito:
//!
//! * [`cfdi`] — arma el XML. No necesita internet ni credenciales, y se prueba
//!   solo con las matemáticas del comprobante.
//! * [`pac`] — deja el XML en una carpeta y recoge el timbre. **Sin HTTP:**
//!   un intento de timbrar por internet se rechaza y no sale de la máquina.
//!
//! El programa nunca inventa un timbre: si el contador todavía no deja el
//! archivo, el comprobante se queda en la cola y se reintenta.

#![forbid(unsafe_code)]
// En el código del comprobante un `unwrap()` es un billete de 500 pesos
// perdido: si algo falla, el programa se cierra en medio de una venta. En las
// pruebas no hay tal riesgo, y prohibirlo haría el test ilegible.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod cancelacion;
pub mod cfdi;
pub mod global;
pub mod pac;
pub mod pago;
