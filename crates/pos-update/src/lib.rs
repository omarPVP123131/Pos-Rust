//! Actualizaciones firmadas con respaldo y marcha atrás.
//!
//! Reglas de este crate, en el idioma del mostrador:
//!
//! 1. **Nada se instala si no viene firmado.** La llave pública va dentro del
//!    binario; la privada nunca sale de la máquina donde se firma.
//! 2. **Nada se borra hasta que lo nuevo ha servido.** El ejecutable que sí
//!    funcionaba se guarda antes de tocar nada.
//! 3. **Si el nuevo no arranca, se deshace solo.** Y si se corta la luz en
//!    medio, queda un marcador que lo dice.
//! 4. **Nunca se baja de versión** y no se saltan versiones que el paquete
//!    declare obligatorias.
//!
//! El formato del paquete y su verificación están en [`paquete`]; el mecanismo
//! de respaldo/cambio/marcha atrás, en [`aplicar`].

pub mod aplicar;
pub mod paquete;

use std::fmt;

/// Versión del programa. **Un solo lugar**: de aquí la lee el empaquetado
/// (`packaging/construir.sh`) y contra ésta se comparan los paquetes.
pub const VERSION: &str = "0.6.2";

/// La llave pública va quemada en el binario: cambiarla exige recompilar.
const LLAVE_PUBLICA: &str = include_str!("../llaves/pos-publica.txt");

pub type Result<T> = std::result::Result<T, Error>;

/// Por qué un paquete no se puede instalar. Los mensajes son para el encargado,
/// no para el programador.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// El archivo no es un paquete, o está incompleto.
    Formato(String),
    /// El ejecutable del paquete no corresponde al hash firmado.
    HashIncorrecto,
    /// La firma no corresponde: o no es nuestro, o lo alteraron.
    FirmaInvalida,
    /// La llave quemada en el binario no sirve (error de compilación, no del paquete).
    LlaveMala,
    /// El paquete trae una versión igual o menor que la instalada.
    VersionVieja { actual: String, paquete: String },
    /// Hay que subir primero a otra versión.
    MuyNuevo { actual: String, minimo: String },
    /// No se pudo leer, escribir o reemplazar algo.
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Formato(d) => write!(f, "{d}"),
            Error::HashIncorrecto => write!(
                f,
                "El archivo está alterado: no corresponde a lo que se firmó. No se instala."
            ),
            Error::FirmaInvalida => write!(
                f,
                "La firma no es válida. Este paquete no salió de nosotros: no se instala."
            ),
            Error::LlaveMala => write!(
                f,
                "Este programa se compiló con una llave pública inválida. Reinstálalo."
            ),
            Error::VersionVieja { actual, paquete } => write!(
                f,
                "Ya tienes la {actual}: el paquete trae la {paquete}, que no es más nueva."
            ),
            Error::MuyNuevo { actual, minimo } => write!(
                f,
                "Para subir a esta versión primero hay que tener la {minimo} (tienes la {actual})."
            ),
            Error::Io(d) => write!(f, "{d}"),
        }
    }
}

impl std::error::Error for Error {}

/// La llave pública en bytes. `LlaveMala` si el archivo no trae 32 bytes hex.
pub fn llave_publica() -> Result<[u8; 32]> {
    let texto = LLAVE_PUBLICA.trim();
    let bytes = de_hex(texto).ok_or(Error::LlaveMala)?;
    bytes.try_into().map_err(|_| Error::LlaveMala)
}

/// `de_hex` al revés: minúsculas, sin separadores.
pub fn a_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Acepta mayúsculas y minúsculas; `None` si no es hexadecimal.
pub fn de_hex(texto: &str) -> Option<Vec<u8>> {
    let limpio: String = texto.chars().filter(|c| !c.is_whitespace()).collect();
    if limpio.is_empty() || !limpio.len().is_multiple_of(2) {
        return None;
    }
    (0..limpio.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&limpio[i..i + 2], 16).ok())
        .collect()
}

/// Fecha y hora local (Ciudad de México) para el marcador y la bitácora.
pub fn ahora() -> String {
    pos_core::FechaHora::cdmx(pos_core::ahora_segs()).fecha_hora()
}

/// La llave privada de **desarrollo**, si el archivo está ahí.
///
/// La usan las pruebas de los demás crates para firmar como firmaría quien
/// suelta una versión. La llave de la tienda real se genera aparte con
/// `pos-firmar --generar` y no se comparte con nadie; en una instalación de
/// verdad este archivo no existe y la función da `None`.
pub fn llave_privada_de_desarrollo() -> Option<[u8; 32]> {
    // La ruta se resuelve al compilar: sirve desde cualquier directorio.
    let ruta = concat!(env!("CARGO_MANIFEST_DIR"), "/llaves/pos-privada.txt");
    let texto = std::fs::read_to_string(ruta).ok()?;
    de_hex(texto.trim())?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_hex_da_la_vuelta() {
        assert_eq!(a_hex(&[0x00, 0x0f, 0xff]), "000fff");
        assert_eq!(de_hex("000fff").unwrap(), vec![0x00, 0x0f, 0xff]);
        // Con mayúsculas y espacios, como lo pega alguien a mano.
        assert_eq!(de_hex("00 0F FF").unwrap(), vec![0x00, 0x0f, 0xff]);
        assert_eq!(de_hex("xyz"), None);
        assert_eq!(de_hex("abc"), None, "longitud nones");
        assert_eq!(de_hex("").unwrap_or_default().len(), 0);
    }

    #[test]
    fn la_version_tiene_formato_de_tres_numeros() {
        let p = paquete::partes(VERSION);
        assert!(p.0 > 0 || p.1 > 0 || p.2 > 0);
        assert_eq!(VERSION.split('.').count(), 3);
    }

    #[test]
    fn la_llave_publica_del_binario_es_una_llave_de_verdad() {
        // Si esto falla, el programa se compiló con el archivo de ejemplo y
        // no aceptaría ningún paquete.
        let k = llave_publica().expect("la llave pública debe ser hexadecimal válido");
        let v = ed25519_dalek::VerifyingKey::from_bytes(&k);
        assert!(v.is_ok(), "la llave pública no es un punto válido");
    }

    #[test]
    fn los_errores_se_explican_solos() {
        assert!(Error::FirmaInvalida
            .to_string()
            .contains("no salió de nosotros"));
        assert!(Error::HashIncorrecto.to_string().contains("alterado"));
        assert!(Error::VersionVieja {
            actual: "0.5.0".into(),
            paquete: "0.4.0".into()
        }
        .to_string()
        .contains("Ya tienes la 0.5.0"));
    }

    #[test]
    fn la_hora_sale_en_formato_legible() {
        let f = ahora();
        assert!(f.len() >= 16, "{f}");
        assert!(f.contains('-') && f.contains(':'), "{f}");
    }
}
