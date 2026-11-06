//! Constructor de tickets ESC/POS.
//!
//! No imprime: **fabrica los bytes**. Eso permite probar el ticket sin tener
//! una impresora enfrente (comparando contra bytes esperados) y mostrar una
//! vista previa en pantalla con `--dry-run`.
//!
//! El constructor trabaja con un [`Perfil`](crate::perfil::Perfil), que es lo
//! que hace que el mismo ticket salga bien en una Epson y en una genérica
//! china: la página de códigos, el corte y el pin del cajón cambian entre
//! modelos y aquí se respetan.

use crate::codigo::{decodificar, transcribir};
use crate::perfil::Perfil;

pub const ESC: u8 = 0x1B;
pub const GS: u8 = 0x1D;
pub const BEL: u8 = 0x07;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    Partial,
    Full,
}

/// Constructor de un ticket de 58 mm (32 columnas) u 80 mm (48 columnas).
#[derive(Clone, Debug)]
pub struct TicketBuilder {
    out: Vec<u8>,
    width: usize,
    perfil: Perfil,
}

impl TicketBuilder {
    pub fn new(width: usize) -> Self {
        Self::con_perfil(Perfil::generica_80().con_columnas(width))
    }

    pub fn for_58mm() -> Self {
        Self::con_perfil(Perfil::generica_58())
    }

    pub fn for_80mm() -> Self {
        Self::con_perfil(Perfil::generica_80())
    }

    /// Construye con un perfil concreto: es la forma correcta de armar un
    /// ticket cuando se sabe qué impresora hay enfrente.
    pub fn con_perfil(perfil: Perfil) -> Self {
        let width = perfil.columnas;
        let mut t = TicketBuilder {
            out: Vec::with_capacity(1024),
            width,
            perfil,
        };
        if t.perfil.init {
            t.raw(&[ESC, b'@']); // inicializar impresora
        }
        // Antes del primer carácter hay que decir la página de códigos: si no,
        // la impresora usa la de fábrica y los acentos se pierden.
        let pagina = t.perfil.bytes_pagina();
        if !pagina.is_empty() {
            t.raw(&pagina);
        }
        t
    }

    /// El perfil con el que se está construyendo.
    pub fn perfil(&self) -> &Perfil {
        &self.perfil
    }

    fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.out.extend_from_slice(bytes);
        self
    }

    pub fn align(&mut self, a: Align) -> &mut Self {
        let n = match a {
            Align::Left => b'0',
            Align::Center => b'1',
            Align::Right => b'2',
        };
        self.raw(&[ESC, b'a', n]);
        self
    }

    pub fn bold(&mut self, on: bool) -> &mut Self {
        self.raw(&[ESC, b'E', u8::from(on)])
    }

    /// Texto doble alto y ancho: para el total y el encabezado.
    pub fn double(&mut self, on: bool) -> &mut Self {
        let n = if on { 0x30 } else { 0x00 };
        self.raw(&[GS, b'!', n])
    }

    pub fn underline(&mut self, on: bool) -> &mut Self {
        self.raw(&[ESC, b'-', u8::from(on)])
    }

    /// Escribe una línea de texto (se transcribe a la página del perfil).
    pub fn line(&mut self, texto: &str) -> &mut Self {
        let bytes = transcribir(texto, self.perfil.pagina);
        self.raw(&bytes);
        self.raw(b"\n")
    }

    /// Línea con puntos: "Coca-Cola 600ml ..... $18.00"
    pub fn kv(&mut self, izquierda: &str, derecha: &str) -> &mut Self {
        let ancho = self.width.max(8);
        let izq = transcribir(izquierda, self.perfil.pagina);
        let der = transcribir(derecha, self.perfil.pagina);
        let espacios = ancho.saturating_sub(izq.len() + der.len()).max(1);
        let mut linea = izq;
        linea.extend(vec![b'.'; espacios]);
        linea.extend(der);
        // Si se pasó de ancho, truncamos para que la impresora no haga salto feo.
        linea.truncate(ancho + 8);
        self.raw(&linea);
        self.raw(b"\n")
    }

    pub fn separator(&mut self) -> &mut Self {
        self.raw(&vec![b'-'; self.width]);
        self.raw(b"\n")
    }

    pub fn feed(&mut self, lineas: u8) -> &mut Self {
        self.raw(&[ESC, b'd', lineas])
    }

    /// Pulso al cajón de dinero (puerto RJ11 de la impresora).
    ///
    /// El pin lo decide el perfil: casi todas abren con el 2, pero Citizen y
    /// algunas Star traen el cajón en el 5.
    pub fn open_drawer(&mut self) -> &mut Self {
        let cmd = self.perfil.bytes_cajon();
        if !cmd.is_empty() {
            self.raw(&cmd);
        }
        self
    }

    /// Pitido corto (muy útil para confirmar lectura de código de barras).
    pub fn beep(&mut self) -> &mut Self {
        self.raw(&[BEL])
    }

    /// Código QR modelo 2, el que leen los lectores del SAT y del celular.
    ///
    /// Si el perfil dice que la impresora no trae QR, no se manda: una
    /// impresora vieja lo imprime como una lluvia de caracteres raros.
    pub fn qr(&mut self, data: &str) -> &mut Self {
        let bytes = data.as_bytes();
        if !self.perfil.qr || bytes.is_empty() || bytes.len() > 7000 {
            return self;
        }
        // Modelo 2
        self.raw(&[GS, b'(', b'k', 4, 0, 49, 65, 50, 0]);
        // Tamaño del módulo
        self.raw(&[GS, b'(', b'k', 3, 0, 49, 67, 8]);
        // Nivel de corrección de errores (M)
        self.raw(&[GS, b'(', b'k', 3, 0, 49, 69, 49]);
        // Guardar datos: pL,pH = len(data) + 3
        let len = bytes.len() + 3;
        let pl = (len & 0xFF) as u8;
        let ph = (len >> 8) as u8;
        let mut cmd = vec![GS, b'(', b'k', pl, ph, 49, 80, 48];
        cmd.extend_from_slice(bytes);
        self.raw(&cmd);
        // Imprimir
        self.raw(&[GS, b'(', b'k', 3, 0, 49, 81, 48])
    }

    /// Código de barras EAN-13 (13 dígitos exactos).
    pub fn barcode_ean13(&mut self, code: &str) -> &mut Self {
        let digitos: String = code
            .chars()
            .filter(|c| c.is_ascii_digit())
            .take(13)
            .collect();
        if digitos.len() != 13 {
            return self;
        }
        let mut cmd = vec![GS, b'k', 67, 13];
        cmd.extend_from_slice(digitos.as_bytes());
        self.raw(&cmd)
    }

    /// Corte con un modo explícito (no usa el perfil).
    pub fn cut(&mut self, modo: Cut) -> &mut Self {
        let n = match modo {
            Cut::Partial => b'1',
            Cut::Full => b'0',
        };
        self.raw(&[GS, b'V', n])
    }

    /// Termina el ticket como manda el perfil: deja papel, corta (o sólo
    /// alimenta, si no hay cuchilla) y deja la impresora lista.
    ///
    /// Éste es el que debe usar la aplicación: [`Self::cut`] queda para los
    /// casos en que se quiere forzar un modo concreto.
    pub fn cortar(&mut self) -> &mut Self {
        self.feed(self.perfil.lineas_finales);
        let cmd = self.perfil.bytes_corte();
        if !cmd.is_empty() {
            self.raw(&cmd);
        }
        self
    }

    pub fn build(self) -> Vec<u8> {
        self.out
    }

    /// Vista previa legible: para depurar sin impresora.
    ///
    /// Se decodifica con la página de códigos del perfil: si se leyera como
    /// UTF-8, los acentos saldrían como basura justo en la pantalla donde el
    /// cajero revisa el ticket.
    pub fn build_preview(&self) -> String {
        decodificar(&strip_commands(&self.out), self.perfil.pagina)
    }
}

/// Etiqueta de anaquel: nombre, precio y EAN-13. El lector lo cobra igual.
pub fn etiqueta_ean13(perfil: &Perfil, nombre: &str, precio: &str, codigo: &str) -> Vec<u8> {
    let mut t = TicketBuilder::con_perfil(perfil.clone());
    t.align(Align::Center)
        .bold(true)
        .line(nombre)
        .bold(false)
        .double(true)
        .line(precio)
        .double(false)
        .barcode_ean13(codigo)
        .line(codigo)
        .cortar();
    t.build()
}

/// Ticket de prueba: lo que se imprime para saber si el perfil está bien.
///
/// No es un adorno. Trae las cuatro cosas que hay que mirar en el mostrador:
///   1. una regla de `0` a `9` para ver si el ancho es el correcto;
///   2. los acentos del español, para ver si la página de códigos es la buena;
///   3. un QR, para saber si la impresora lo soporta;
///   4. el corte, para comprobar que la cuchilla responde.
pub fn ticket_de_prueba(perfil: &Perfil, tienda: &str, puerto: &str, abrir_cajon: bool) -> Vec<u8> {
    let mut t = TicketBuilder::con_perfil(perfil.clone());
    let ancho = perfil.columnas;

    t.align(Align::Center).bold(true).line(tienda).bold(false);
    t.line("PRUEBA DE IMPRESORA").separator();

    t.align(Align::Left);
    t.line("Espanol: á é í ó ú ñ Ñ");
    t.line("Mayusculas: Á É Í Ó Ú");
    t.line("Signos: ¿ ¡ ° º ª $ %");
    t.line(&format!("Pagina de codigos: {}", perfil.pagina.texto()));
    t.line(&format!("Puerto: {puerto}"));
    t.separator();

    // Regla: si se corta antes del final, el ancho está mal puesto.
    let regla: String = (1..=ancho)
        .map(|i| char::from(b'0' + (i % 10) as u8))
        .collect();
    t.line(&format!("Ancho: {ancho} columnas"));
    t.line(&regla);
    t.separator();

    t.align(Align::Center)
        .double(true)
        .line("SI LEE ESTO")
        .line("EL ANCHO ESTA BIEN")
        .double(false);

    if perfil.qr {
        t.align(Align::Center)
            .line("(y este es un QR)")
            .qr("https://pos.local/prueba");
    } else {
        t.align(Align::Center)
            .line("(tu impresora no trae QR y no se lo mandamos)");
    }

    t.align(Align::Center).line("¿Salieron los acentos?");
    t.line("Si salen raros, cambia la");
    t.line("pagina de codigos en Ajustes.");

    if abrir_cajon {
        t.open_drawer();
    }
    t.cortar();
    t.build()
}

/// Quita todos los comandos ESC/POS y deja solo el texto imprimible.
///
/// Sirve para la vista previa en pantalla y para las pruebas: así se puede
/// medir el ancho real de cada línea del ticket sin contar los comandos.
pub fn strip_commands(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            ESC => {
                if i + 1 >= bytes.len() {
                    break;
                }
                let parametros = match bytes[i + 1] {
                    b'@' => 0,
                    b'a' | b'E' | b'-' | b'd' | b'!' | b't' => 1,
                    b'p' => 3,
                    _ => 0,
                };
                i += 2 + parametros;
            }
            GS => {
                if i + 1 >= bytes.len() {
                    break;
                }
                match bytes[i + 1] {
                    b'!' => i += 3,
                    // GS V 0/1 son 3 bytes; GS V B n (alimenta y corta) son 4.
                    b'V' if i + 2 < bytes.len() => i += if bytes[i + 2] == b'B' { 4 } else { 3 },
                    b'k' => {
                        // Código de barras: longitud desconocida, saltamos
                        // hasta el siguiente comando o salto de línea.
                        i += 2;
                        while i < bytes.len()
                            && bytes[i] != ESC
                            && bytes[i] != GS
                            && bytes[i] != b'\n'
                        {
                            i += 1;
                        }
                    }
                    b'(' if i + 4 < bytes.len() && bytes[i + 2] == b'k' => {
                        let len = bytes[i + 3] as usize | ((bytes[i + 4] as usize) << 8);
                        i += 5 + len;
                    }
                    _ => i += 2,
                }
            }
            BEL => i += 1,
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// UTF-8 (español) -> CP437, el juego de caracteres de las térmicas genéricas.
pub fn to_cp437(texto: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(texto.len());
    for ch in texto.chars() {
        let byte = match ch {
            'á' | 'Á' => 0xA0,
            'í' | 'Í' => 0xA1,
            'ó' | 'Ó' => 0xA2,
            'ú' | 'Ú' => 0xA3,
            'ñ' => 0xA4,
            'Ñ' => 0xA5,
            'é' => 0x82,
            'É' => 0x90,
            'ü' => 0x81,
            'Ü' => 0x9A,
            'ö' => 0x94,
            'ä' => 0x84,
            'à' => 0x85,
            'â' => 0x83,
            'ê' => 0x88,
            'ë' => 0x89,
            'è' => 0x8A,
            'ï' => 0x8B,
            'î' => 0x8C,
            'ì' => 0x8D,
            'ô' => 0x93,
            'ò' => 0x95,
            'û' => 0x96,
            'ù' => 0x97,
            'ç' => 0x87,
            'Ç' => 0x80,
            '¿' => 0xA8,
            '¡' => 0xAD,
            '°' => 0xF8,
            'º' => 0xA7,
            'ª' => 0xA6,
            '¢' => 0x9B,
            '£' => 0x9C,
            '¥' => 0x9D,
            c if c.is_ascii() && c != '\0' => c as u8,
            _ => b'?',
        };
        out.push(byte);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_ticket_empieza_por_init_y_termina_en_corte() {
        let mut t = TicketBuilder::for_80mm();
        t.line("Hola").cut(Cut::Partial);
        let bytes = t.build();
        assert_eq!(&bytes[0..2], &[ESC, b'@']);
        let final_ = &bytes[bytes.len() - 3..];
        assert_eq!(final_, &[GS, b'V', b'1']);
    }

    #[test]
    fn el_ticket_se_arma_completo() {
        let mut t = TicketBuilder::for_80mm();
        t.align(Align::Center)
            .bold(true)
            .line("Abarrotes Lupita")
            .bold(false)
            .separator()
            .kv("2 x Coca-Cola 600ml", "$36.00")
            .kv("1 x Pan blanco", "$18.50")
            .separator()
            .align(Align::Right)
            .kv("TOTAL", "$54.50")
            .qr("https://mitienda.mx/t/000123")
            .feed(3)
            .cut(Cut::Partial);
        let bytes = t.build();

        let texto = String::from_utf8_lossy(&bytes);
        assert!(texto.contains("Abarrotes Lupita"));
        assert!(texto.contains("$54.50"));
        // El QR se emitió (secuencia GS ( k ... 81 48)
        assert!(bytes.windows(3).any(|w| w == [GS, b'(', b'k']));
        assert!(bytes.windows(3).any(|w| w == [49, 81, 48]));
    }

    #[test]
    fn los_acentos_y_la_enie_llegan_bien_a_la_impresora() {
        assert_eq!(to_cp437("ñ"), vec![0xA4]);
        assert_eq!(to_cp437("Ñ"), vec![0xA5]);
        assert_eq!(to_cp437("jamón"), vec![b'j', b'a', b'm', 0xA2, b'n']);
        assert_eq!(to_cp437("¿qué?"), vec![0xA8, b'q', b'u', 0x82, b'?']);
        // Un emoji no rompe el ticket: se sustituye por '?'
        assert_eq!(
            to_cp437("café ☕"),
            vec![b'c', b'a', b'f', 0x82, b' ', b'?']
        );
    }

    #[test]
    fn el_cajon_y_el_pitido_son_comandos_estandar() {
        let mut t = TicketBuilder::for_58mm();
        t.open_drawer().beep();
        let bytes = t.build();
        assert!(bytes.windows(5).any(|w| w == [ESC, b'p', 0, 25, 250]));
        assert!(bytes.contains(&BEL));
    }

    #[test]
    fn el_ean13_exige_13_digitos() {
        let mut t = TicketBuilder::for_80mm();
        t.barcode_ean13("7501234567890"); // 13 dígitos
        t.barcode_ean13("123"); // inválido: se ignora
        let bytes = t.build();
        let cuenta = bytes.windows(3).filter(|w| *w == [GS, b'k', 67]).count();
        assert_eq!(cuenta, 1);
    }

    #[test]
    fn la_etiqueta_lleva_nombre_precio_y_codigo() {
        let bytes = etiqueta_ean13(
            &Perfil::generica_80(),
            "Arroz a granel",
            "$25.00",
            "2000000000015",
        );
        assert!(bytes.windows(3).any(|w| w == [GS, b'k', 67]));
        let texto = decodificar(&strip_commands(&bytes), crate::codigo::Pagina::Cp850);
        assert!(texto.contains("Arroz a granel"), "{texto}");
        assert!(texto.contains("$25.00"), "{texto}");
        // El EAN viaja como comando, no como texto: el lector lo ve, el
        // stripper de la vista previa se lo come.
    }

    #[test]
    fn el_stripper_deja_solo_el_texto_imprimible() {
        let mut t = TicketBuilder::for_58mm();
        t.align(Align::Left)
            .bold(true)
            .line("Hola")
            .beep()
            .cut(Cut::Full);
        let texto = String::from_utf8_lossy(&strip_commands(&t.build())).to_string();
        assert_eq!(texto.trim(), "Hola");
    }

    #[test]
    fn el_ticket_de_prueba_trae_las_cuatro_cosas_que_hay_que_mirar() {
        let perfil = Perfil::generica_80();
        let bytes = ticket_de_prueba(&perfil, "Abarrotes Lupita", "/dev/usb/lp0", true);
        let texto = decodificar(&strip_commands(&bytes), perfil.pagina);

        // 1. La regla con el ancho real del papel.
        assert!(texto.contains("Ancho: 48 columnas"), "sin regla: {texto}");
        assert!(texto.contains("12345678901234567890"));
        // 2. Los acentos y la ñ (y no como '?').
        assert!(
            texto.contains("á é í ó ú ñ Ñ"),
            "se perdieron acentos: {texto}"
        );
        assert!(
            texto.contains("Á É Í Ó Ú"),
            "se perdieron mayúsculas: {texto}"
        );
        // 3. El QR.
        assert!(bytes.windows(3).any(|w| w == [GS, b'(', b'k']));
        // 4. El pulso al cajón y el corte del perfil.
        assert!(bytes.windows(5).any(|w| w == [ESC, b'p', 0, 25, 250]));
        assert!(bytes.windows(3).any(|w| w == [GS, b'V', b'1']));
        // Y dice por dónde salió.
        assert!(texto.contains("/dev/usb/lp0"));
    }

    #[test]
    fn el_ticket_de_prueba_se_adapta_al_perfil() {
        // Sin cuchilla: no se manda ningún corte.
        let bytes = ticket_de_prueba(&Perfil::sin_cuchilla(), "T", "x", false);
        assert!(!bytes.windows(3).any(|w| w == [GS, b'V', b'1']));
        assert!(!bytes.windows(3).any(|w| w == [GS, b'V', b'B']));
        // Sin QR: ni una sola secuencia GS ( k.
        let bytes = ticket_de_prueba(&Perfil::sin_qr(), "T", "x", false);
        assert!(!bytes.windows(3).any(|w| w == [GS, b'(', b'k']));
        // Y tampoco abre el cajón si no se pidió.
        assert!(!bytes.windows(5).any(|w| w == [ESC, b'p', 0, 25, 250]));
    }

    #[test]
    fn el_ticket_de_prueba_cabe_en_58_mm() {
        let perfil = Perfil::generica_58();
        let bytes = ticket_de_prueba(&perfil, "Tiendita", "/dev/ttyUSB0", false);
        let texto = decodificar(&strip_commands(&bytes), perfil.pagina);
        for linea in texto.lines() {
            assert!(
                linea.chars().count() <= 32 + 8,
                "línea larga ({}) en 58 mm: {linea}",
                linea.chars().count()
            );
        }
    }

    #[test]
    fn la_linea_con_puntos_no_rebasa_el_ancho() {
        let mut t = TicketBuilder::for_58mm(); // 32 columnas
        t.kv("Producto con un nombre bastante largo", "$1,234.56");
        let bytes = t.build();
        // Se mide lo que se imprime, sin contar los comandos ESC/POS.
        let impreso = strip_commands(&bytes);
        let texto = String::from_utf8_lossy(&impreso);
        for linea in texto.lines() {
            assert!(linea.chars().count() <= 32 + 8, "línea larga: {linea}");
        }
    }
}
