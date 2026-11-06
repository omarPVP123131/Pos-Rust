//! `pos-hw` — periféricos de mostrador.
//!
//! Dos ideas importantes:
//!   1. El ticket se construye como bytes, así que se prueba **sin impresora**.
//!   2. El escáner de código de barras es HID: se conecta como teclado, escribe
//!      el código y manda Enter. Cero drivers, cero dependencias.

pub mod codigo;
pub mod detectar;
pub mod escpos;
pub mod lan;
pub mod perfil;
pub mod printer;
pub mod receipt;
pub mod transporte;
#[cfg(windows)]
pub mod winspool;

pub use codigo::{decodificar, transcribir, Pagina};
pub use detectar::{
    consejo_error_spooler, enumerar, es_cola_virtual, mejor, puntuacion_termica, ImpresoraDetectada,
};
pub use escpos::{etiqueta_ean13, strip_commands, ticket_de_prueba, Cut, TicketBuilder};
pub use lan::direcciones_lan;
pub use perfil::{normalizar_baudios, Corte, Perfil, PinCajon, BAUDIOS};
pub use printer::{detectar_impresora, FilePrinter, HwError, HwResult, NullPrinter, Printer};
pub use receipt::{ReceiptData, ReceiptItem};
pub use transporte::{Transporte, PUERTO_RAW};
