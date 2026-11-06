//! Salida física del ticket.
//!
//! En Linux una impresora USB de clase impresora aparece como `/dev/usb/lp0`:
//! escribir ahí es ESC/POS directo, sin drivers ni CUPS (que en un Celeron con
//! 1 GB de RAM es un lujo innecesario).
//! En Windows el camino equivalente es el nombre del puerto (`USB001`) y se
//! añade en la Fase 4 con la API de spooler.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug)]
pub enum HwError {
    Io(std::io::Error),
    NoDisponible(String),
    /// El puerto existe pero algo salió mal al hablar con la impresora
    /// (no abre, se desconectó, baudios imposibles...). El mensaje viene
    /// escrito para el encargado de la tienda, no para el programador.
    Puerto(String),
}

impl From<std::io::Error> for HwError {
    fn from(e: std::io::Error) -> Self {
        HwError::Io(e)
    }
}

impl std::fmt::Display for HwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HwError::Io(e) => write!(f, "Error de impresión: {e}"),
            HwError::NoDisponible(s) => write!(f, "Impresora no disponible: {s}"),
            HwError::Puerto(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for HwError {}

pub type HwResult<T> = Result<T, HwError>;

pub trait Printer {
    fn name(&self) -> &str;
    fn write(&mut self, bytes: &[u8]) -> HwResult<()>;
}

/// Escribe los bytes crudos en un archivo/dispositivo (Linux: `/dev/usb/lp0`).
pub struct FilePrinter {
    path: PathBuf,
}

impl FilePrinter {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FilePrinter { path: path.into() }
    }
}

impl Printer for FilePrinter {
    fn name(&self) -> &str {
        self.path.to_str().unwrap_or("archivo")
    }

    fn write(&mut self, bytes: &[u8]) -> HwResult<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(bytes)?;
        f.flush()?;
        Ok(())
    }
}

/// Para `--dry-run`, pruebas y para vender sin impresora conectada.
pub struct NullPrinter {
    ultimo: Vec<u8>,
}

impl NullPrinter {
    pub fn new() -> Self {
        NullPrinter { ultimo: Vec::new() }
    }

    pub fn ultimo_ticket(&self) -> &[u8] {
        &self.ultimo
    }
}

impl Default for NullPrinter {
    fn default() -> Self {
        Self::new()
    }
}

impl Printer for NullPrinter {
    fn name(&self) -> &str {
        "sin impresora"
    }

    fn write(&mut self, bytes: &[u8]) -> HwResult<()> {
        self.ultimo = bytes.to_vec();
        Ok(())
    }
}

/// Primera impresora que parezca una térmica.
///
/// Se conserva por compatibilidad (la UI la llamaba antes), pero hoy el camino
/// bueno es [`crate::detectar::enumerar`], que no adivina: **mira** el equipo,
/// dice si es USB o serie y de qué fabricante es.
pub fn detectar_impresora() -> Option<PathBuf> {
    crate::detectar::enumerar()
        .into_iter()
        .next()
        .map(|i| PathBuf::from(i.ruta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_impresora_nula_guarda_el_ticket_para_inspeccionarlo() {
        let mut p = NullPrinter::new();
        p.write(&[crate::escpos::ESC, b'@', b'A']).unwrap();
        assert_eq!(p.ultimo_ticket(), &[crate::escpos::ESC, b'@', b'A']);
    }

    #[test]
    fn la_impresora_de_archivo_escribe_en_append() {
        let dir = std::env::temp_dir().join("pos-hw-test");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("ticket.bin");
        let _ = std::fs::remove_file(&ruta);

        {
            let mut p = FilePrinter::new(&ruta);
            p.write(b"uno").unwrap();
            p.write(b"dos").unwrap();
        }
        assert_eq!(std::fs::read(&ruta).unwrap(), b"unodos");
        std::fs::remove_file(&ruta).unwrap();
    }
}
