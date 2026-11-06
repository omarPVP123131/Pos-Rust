//! Impresión RAW y listado de impresoras en Windows.
//!
//! Se usa la API del spooler directamente (`winspool.drv`), sin dependencias
//! externas: nada de `winapi`, nada de `windows-sys`, que son dos subidas de
//! peso que no caben en el espíritu de este programa.
//!
//! La clave de todo es el tipo de datos **RAW**: le dice al spooler que no
//! interprete los bytes. Sin RAW, Windows intenta dibujar el ticket como si
//! fuera un documento y sale una hoja llena de símbolos.

#![cfg(windows)]

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::ptr;

type Bool = i32;
type Handle = *mut core::ffi::c_void;

const PRINTER_ENUM_LOCAL: u32 = 0x0000_0002;
const PRINTER_ENUM_CONNECTIONS: u32 = 0x0000_0004;

#[repr(C)]
struct InfoImpresora4 {
    nombre: *const u16,
    servidor: *const u16,
    atributos: u32,
}

#[repr(C)]
struct DocInfo1 {
    documento: *const u16,
    salida: *const u16,
    tipo: *const u16,
}

#[link(name = "winspool")]
extern "system" {
    fn EnumPrintersW(
        banderas: u32,
        nombre: *const u16,
        nivel: u32,
        buffer: *mut u8,
        tamano: u32,
        necesarios: *mut u32,
        devueltos: *mut u32,
    ) -> Bool;

    fn OpenPrinterW(nombre: *const u16, impresora: *mut Handle, por_omision: *mut u8) -> Bool;

    fn StartDocPrinterW(impresora: Handle, nivel: u32, documento: *const DocInfo1) -> u32;
    fn EndDocPrinter(impresora: Handle) -> Bool;
    fn StartPagePrinter(impresora: Handle) -> Bool;
    fn EndPagePrinter(impresora: Handle) -> Bool;
    fn WritePrinter(impresora: Handle, buffer: *const u8, bytes: u32, escritos: *mut u32) -> Bool;
    fn ClosePrinter(impresora: Handle) -> Bool;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
}

/// Convierte una cadena de Rust a UTF-16 terminada en cero.
fn a_utf16(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn de_utf16(ptr: *const u16) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut fin = 0usize;
    unsafe {
        while *ptr.add(fin) != 0 {
            fin += 1;
        }
        Some(
            OsString::from_wide(std::slice::from_raw_parts(ptr, fin))
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// Nombres de las impresoras instaladas en la máquina.
pub fn listar() -> Vec<String> {
    unsafe {
        let mut necesarios: u32 = 0;
        let mut devueltos: u32 = 0;
        // Primera llamada: sólo pregunta cuánta memoria hace falta.
        let _ = EnumPrintersW(
            PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
            ptr::null(),
            4,
            ptr::null_mut(),
            0,
            &mut necesarios,
            &mut devueltos,
        );
        if necesarios == 0 {
            return Vec::new();
        }
        let mut buffer = vec![0u8; necesarios as usize];
        let ok = EnumPrintersW(
            PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
            ptr::null(),
            4,
            buffer.as_mut_ptr(),
            necesarios,
            &mut necesarios,
            &mut devueltos,
        );
        if ok == 0 {
            return Vec::new();
        }
        let info = buffer.as_ptr() as *const InfoImpresora4;
        (0..devueltos)
            .filter_map(|i| de_utf16((*info.add(i as usize)).nombre))
            .collect()
    }
}

/// Manda bytes ESC/POS en crudo a una impresora de Windows.
pub fn escribir_raw(impresora: &str, bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let nombre = a_utf16(impresora);
        let mut h: Handle = ptr::null_mut();
        if OpenPrinterW(nombre.as_ptr(), &mut h, ptr::null_mut()) == 0 {
            let codigo = GetLastError();
            return Err(crate::detectar::consejo_error_spooler(codigo, impresora));
        }

        let doc = a_utf16("Ticket POS");
        let tipo = a_utf16("RAW"); // sin esto Windows interpreta el ticket
        let info = DocInfo1 {
            documento: doc.as_ptr(),
            salida: ptr::null(),
            tipo: tipo.as_ptr(),
        };

        let mut escritos: u32 = 0;
        let exito = StartDocPrinterW(h, 1, &info) != 0
            && StartPagePrinter(h) != 0
            && WritePrinter(h, bytes.as_ptr(), bytes.len() as u32, &mut escritos) != 0
            && EndPagePrinter(h) != 0
            && EndDocPrinter(h) != 0;

        ClosePrinter(h);

        if exito && escritos as usize == bytes.len() {
            Ok(())
        } else if escritos != bytes.len() as u32 {
            Err(format!(
                "la impresora sólo aceptó {escritos} de {} bytes",
                bytes.len()
            ))
        } else {
            Err(format!("«{impresora}» rechazó el trabajo de impresión"))
        }
    }
}
