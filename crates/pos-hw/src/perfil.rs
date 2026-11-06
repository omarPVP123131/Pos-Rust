//! Perfil de la impresora: lo que de verdad cambia entre una térmica y otra.
//!
//! Todas las térmicas hablan ESC/POS, pero **no hablan el mismo dialecto**.
//! Estas son las seis cosas que cambian y que rompen un ticket:
//!
//!   1. **Ancho**: 58 mm (32 columnas) u 80 mm (48 columnas).
//!   2. **Página de códigos**: CP437 (sin Á Í Ó Ú) o CP850 (español completo).
//!   3. **Corte**: `GS V 0`, `GS V 1`, `GS V B` o nada (las de 58 mm baratas
//!      no tienen cuchilla: se alimenta papel y se arranca a mano).
//!   4. **Cajón**: pin 2 o pin 5 del RJ11.
//!   5. **Velocidad**: 9600 en las genéricas chinas, 115200 en Epson/Bixolon.
//!   6. **QR**: las muy viejas no lo traen y lo imprimen como basura.
//!
//! Con eso cubrimos lo que se encuentra en un mostrador mexicano. Lo demás
//! (densidad de impresión, logotipos, modos OPOS por switch) se queda fuera a
//! propósito: es configuración del aparato, no del programa.

use crate::codigo::Pagina;

/// Cómo se corta el papel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corte {
    /// `GS V 1` — corte parcial (deja una pestaña). El más compatible.
    Parcial,
    /// `GS V 0` — corte completo.
    Completo,
    /// `GS V B n` — alimenta n líneas y corta completo. Es el que recomienda
    /// Epson y el único que obedecen varias máquinas modernas.
    AlimentaYCorta(u8),
    /// Sin cuchilla: sólo alimenta `n` líneas para que se alcance el papel.
    Ninguno(u8),
}

/// A qué pin del RJ11 está conectado el cajón de dinero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinCajon {
    /// `ESC p 0` — el de fábrica en casi todas.
    Pin2,
    /// `ESC p 1` — Citizen, algunas Star y varios clones.
    Pin5,
    /// No hay cajón conectado.
    Ninguno,
}

/// Perfil de una impresora térmica concreta.
#[derive(Clone, Debug, PartialEq)]
pub struct Perfil {
    /// Nombre que se muestra (y se guarda) en la configuración.
    pub nombre: String,
    /// Columnas de texto del papel.
    pub columnas: usize,
    /// Página de códigos para los acentos.
    pub pagina: Pagina,
    /// Comando de corte.
    pub corte: Corte,
    /// ¿La impresora soporta QR (`GS ( k`)?
    pub qr: bool,
    /// Pin del cajón.
    pub cajon: PinCajon,
    /// Baudios sugeridos (sólo aplican en puerto serie).
    pub baudios: u32,
    /// Bytes por escritura. Los adaptadores chinos se atragantan con un ticket
    /// entero de un solo golpe.
    pub trozo: usize,
    /// Pausa en milisegundos entre trozos (0 = no esperar).
    pub pausa_ms: u64,
    /// Líneas en blanco antes del corte (para que el corte no coma el texto).
    pub lineas_finales: u8,
    /// ¿Mandar `ESC @` al empezar? Algunas impresoras se "despiertan" mejor.
    pub init: bool,
}

impl Default for Perfil {
    fn default() -> Self {
        Perfil::generica_80()
    }
}

impl Perfil {
    /// Genérica de 80 mm: el perfil por omisión, el que usan el grueso de las
    /// térmicas chinas (POS-8022, Xprinter, Zjiang, Gprinter...).
    pub fn generica_80() -> Self {
        Perfil {
            nombre: "Genérica 80 mm".to_string(),
            columnas: 48,
            pagina: Pagina::Cp850,
            corte: Corte::Parcial,
            qr: true,
            cajon: PinCajon::Pin2,
            baudios: 9600,
            trozo: 256,
            pausa_ms: 0,
            lineas_finales: 4,
            init: true,
        }
    }

    /// Genérica de 58 mm (las de mostrador chico, tipo POS-5890).
    pub fn generica_58() -> Self {
        Perfil {
            nombre: "Genérica 58 mm".to_string(),
            columnas: 32,
            ..Perfil::generica_80()
        }
    }

    /// Igual que la genérica de 80 pero sin cuchilla: se arranca a mano.
    pub fn sin_cuchilla() -> Self {
        Perfil {
            nombre: "Sin cuchilla (58 mm)".to_string(),
            columnas: 32,
            corte: Corte::Ninguno(6),
            ..Perfil::generica_80()
        }
    }

    /// Impresora vieja sin QR: mejor no mandarlo que imprimir basura.
    pub fn sin_qr() -> Self {
        Perfil {
            nombre: "Vieja sin QR".to_string(),
            qr: false,
            corte: Corte::AlimentaYCorta(4),
            ..Perfil::generica_80()
        }
    }

    /// Epson TM-T20 / TM-T82 / TM-T88: las más comunes en México.
    /// Serial va a 115200 y el corte recomendado es `GS V B`.
    pub fn epson() -> Self {
        Perfil {
            nombre: "Epson TM (T20/T82/T88)".to_string(),
            columnas: 48,
            pagina: Pagina::Cp850,
            corte: Corte::AlimentaYCorta(4),
            qr: true,
            cajon: PinCajon::Pin2,
            baudios: 115200,
            trozo: 1024,
            pausa_ms: 0,
            lineas_finales: 3,
            init: true,
        }
    }

    /// Star TSP / mPOP: mismos comandos, cajón en pin 2, 9600 en serie.
    pub fn star() -> Self {
        Perfil {
            nombre: "Star TSP / mPOP".to_string(),
            baudios: 9600,
            ..Perfil::epson()
        }
    }

    /// Bixolon SRP-330/350: van rápido (115200).
    pub fn bixolon() -> Self {
        Perfil {
            nombre: "Bixolon SRP".to_string(),
            baudios: 115200,
            ..Perfil::epson()
        }
    }

    /// Citizen CT-S / CBM: el cajón suele estar en el pin 5.
    pub fn citizen() -> Self {
        Perfil {
            nombre: "Citizen CT-S / CBM".to_string(),
            cajon: PinCajon::Pin5,
            baudios: 19200,
            ..Perfil::epson()
        }
    }

    /// Todos los perfiles, en el orden en que se muestran al encargado.
    pub fn todos() -> Vec<Perfil> {
        vec![
            Perfil::generica_80(),
            Perfil::generica_58(),
            Perfil::sin_cuchilla(),
            Perfil::sin_qr(),
            Perfil::epson(),
            Perfil::star(),
            Perfil::bixolon(),
            Perfil::citizen(),
        ]
    }

    /// Busca por nombre. Desconocido → `None` (el llamador decide).
    pub fn por_nombre(nombre: &str) -> Option<Perfil> {
        Perfil::todos()
            .into_iter()
            .find(|p| p.nombre.eq_ignore_ascii_case(nombre.trim()))
    }

    /// Nombres para el combo de la pantalla de configuración.
    pub fn nombres() -> Vec<String> {
        Perfil::todos().into_iter().map(|p| p.nombre).collect()
    }

    /// Ajusta el ancho conservando el resto del perfil.
    pub fn con_columnas(mut self, columnas: usize) -> Self {
        self.columnas = columnas.clamp(20, 64);
        self
    }

    /// Ajusta los baudios (sólo importa en puerto serie).
    pub fn con_baudios(mut self, baudios: u32) -> Self {
        if baudios > 0 {
            self.baudios = baudios;
        }
        self
    }

    /// Pausa entre trozos calculada para no desbordar el búfer de la impresora.
    ///
    /// A 9600 baudios con 8N1 salen ~960 bytes por segundo. Si se mandan 2 KB
    /// de golpe, las genéricas pierden el final del ticket: se come el corte o
    /// la última línea. Con el pausado, el ticket sale completo aunque tarde
    /// un segundo más.
    pub fn pausa_efectiva_ms(&self, baudios: u32) -> u64 {
        if self.pausa_ms > 0 {
            return self.pausa_ms;
        }
        if baudios == 0 {
            return 0;
        }
        let bytes_por_segundo = (baudios / 10).max(1); // 8N1 = 10 bits por byte
        let ms = (self.trozo as u64 * 1000) / bytes_por_segundo as u64;
        // 20 % de margen: los adaptadores chinos no son puntuales.
        ms + ms / 5 + 1
    }

    /// Bytes del comando de corte. Vacío si la impresora no tiene cuchilla.
    pub fn bytes_corte(&self) -> Vec<u8> {
        match self.corte {
            Corte::Parcial => vec![0x1D, b'V', b'1'],
            Corte::Completo => vec![0x1D, b'V', b'0'],
            Corte::AlimentaYCorta(n) => vec![0x1D, b'V', b'B', n.clamp(0, 255)],
            Corte::Ninguno(_) => Vec::new(),
        }
    }

    /// Bytes del pulso al cajón. Vacío si no hay cajón.
    pub fn bytes_cajon(&self) -> Vec<u8> {
        match self.cajon {
            PinCajon::Pin2 => vec![0x1B, b'p', 0, 25, 250],
            PinCajon::Pin5 => vec![0x1B, b'p', 1, 25, 250],
            PinCajon::Ninguno => Vec::new(),
        }
    }

    /// Bytes para seleccionar la página de códigos (`ESC t n`).
    pub fn bytes_pagina(&self) -> Vec<u8> {
        match self.pagina.selector() {
            Some(n) => vec![0x1B, b't', n],
            None => Vec::new(),
        }
    }
}

/// Baudios que se ofrecen en la configuración.
pub const BAUDIOS: [u32; 6] = [9600, 19200, 38400, 57600, 115200, 230400];

/// Baudios estándar más cercano al que pidió el encargado.
pub fn normalizar_baudios(b: u32) -> u32 {
    BAUDIOS
        .iter()
        .copied()
        .min_by_key(|x| x.abs_diff(b))
        .unwrap_or(9600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_perfil_por_omision_es_80mm_con_espanol_completo() {
        let p = Perfil::default();
        assert_eq!(p.columnas, 48);
        assert_eq!(p.pagina, Pagina::Cp850);
    }

    #[test]
    fn los_perfiles_se_encuentran_por_nombre() {
        assert_eq!(
            Perfil::por_nombre("Epson TM (T20/T82/T88)").unwrap(),
            Perfil::epson()
        );
        assert_eq!(
            Perfil::por_nombre("  genérica 58 mm ").unwrap(),
            Perfil::generica_58()
        );
        assert!(Perfil::por_nombre("HP LaserJet").is_none());
        // Todos los nombres del combo existen de verdad.
        for n in Perfil::nombres() {
            assert!(Perfil::por_nombre(&n).is_some(), "falta el perfil {n}");
        }
    }

    #[test]
    fn el_corte_se_traduce_a_bytes_escpos() {
        assert_eq!(Perfil::generica_80().bytes_corte(), vec![0x1D, b'V', b'1']);
        assert_eq!(Perfil::sin_cuchilla().bytes_corte(), Vec::<u8>::new());
        assert_eq!(Perfil::epson().bytes_corte(), vec![0x1D, b'V', b'B', 4]);
    }

    #[test]
    fn el_cajon_cambia_de_pin_segun_el_fabricante() {
        // Pin 2 (0) en las genéricas y Epson...
        assert_eq!(
            Perfil::generica_80().bytes_cajon(),
            vec![0x1B, b'p', 0, 25, 250]
        );
        // ...pin 5 (1) en Citizen.
        assert_eq!(
            Perfil::citizen().bytes_cajon(),
            vec![0x1B, b'p', 1, 25, 250]
        );
        assert_eq!(Perfil::sin_cuchilla().bytes_cajon().len(), 5);
    }

    #[test]
    fn la_pagina_de_codigos_se_manda_al_empezar() {
        assert_eq!(Perfil::generica_80().bytes_pagina(), vec![0x1B, b't', 2]);
        let mut p = Perfil::generica_80();
        p.pagina = Pagina::Cp437;
        assert_eq!(p.bytes_pagina(), vec![0x1B, b't', 0]);
        p.pagina = Pagina::Utf8;
        assert_eq!(p.bytes_pagina(), Vec::<u8>::new());
    }

    #[test]
    fn el_pausado_evita_desbordar_el_buffer_a_9600() {
        let mut p = Perfil::generica_80(); // trozo de 256
        p.pausa_ms = 0;
        // 256 bytes a 9600 baudios = 266 ms + 20 % de margen + 1
        assert_eq!(p.pausa_efectiva_ms(9600), 320);
        // A 115200 el mismo trozo sale 13 veces más rápido.
        assert!(p.pausa_efectiva_ms(115200) < p.pausa_efectiva_ms(9600) / 10);
        // Si el encargado fijó una pausa a mano, se respeta.
        p.pausa_ms = 50;
        assert_eq!(p.pausa_efectiva_ms(9600), 50);
    }

    #[test]
    fn los_baudios_se_normalizan_al_estandar() {
        assert_eq!(normalizar_baudios(9600), 9600);
        assert_eq!(normalizar_baudios(100000), 115200);
        assert_eq!(normalizar_baudios(0), 9600);
    }

    #[test]
    fn el_ancho_se_ajusta_sin_perder_el_resto() {
        let p = Perfil::epson().con_columnas(32);
        assert_eq!(p.columnas, 32);
        assert_eq!(p.corte, Perfil::epson().corte);
        // Y no acepta disparates.
        assert_eq!(Perfil::epson().con_columnas(2).columnas, 20);
    }
}
