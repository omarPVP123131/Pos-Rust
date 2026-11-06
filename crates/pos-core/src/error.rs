//! Errores del dominio. Mensajes pensados para mostrarse al cajero tal cual.

use std::fmt;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// Se intentó cobrar menos de lo que suma la venta.
    PagoInsuficiente { faltante: i64 },
    /// Descuento mayor al importe de la línea o de la venta.
    DescuentoExcesivo,
    /// Cantidad igual o menor a cero.
    CantidadInvalida,
    /// Precio negativo.
    PrecioInvalido,
    /// Conversión de texto a dinero/cantidad.
    MontoInvalido(String),
    /// Venta sin productos.
    VentaVacia,
    /// Operación que requiere autorización de supervisor.
    RequiereSupervisor(&'static str),
    /// PIN con formato inválido (debe ser de 4 a 8 dígitos).
    PinInvalido,
    /// Usuario bloqueado por demasiados intentos fallidos.
    UsuarioBloqueado,
    /// Credenciales incorrectas.
    CredencialesInvalidas,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::PagoInsuficiente { faltante } => {
                write!(f, "Falta por cubrir ${:.2}", *faltante as f64 / 100.0)
            }
            CoreError::DescuentoExcesivo => {
                write!(f, "El descuento es mayor al importe de la venta")
            }
            CoreError::CantidadInvalida => write!(f, "La cantidad debe ser mayor a cero"),
            CoreError::PrecioInvalido => write!(f, "El precio no puede ser negativo"),
            CoreError::MontoInvalido(s) => write!(f, "Monto inválido: {s}"),
            CoreError::VentaVacia => write!(f, "Agrega al menos un producto a la venta"),
            CoreError::RequiereSupervisor(op) => {
                write!(f, "{op} requiere autorización de un supervisor")
            }
            CoreError::PinInvalido => write!(f, "El PIN debe tener entre 4 y 8 dígitos"),
            CoreError::UsuarioBloqueado => {
                write!(
                    f,
                    "Usuario bloqueado por intentos fallidos. Llama al supervisor."
                )
            }
            CoreError::CredencialesInvalidas => write!(f, "PIN incorrecto"),
        }
    }
}

impl std::error::Error for CoreError {}
