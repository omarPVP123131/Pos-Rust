//! Juegos de caracteres: cómo llega un acento a la impresora.
//!
//! Éste es el punto donde más se rompen los tickets en México. Una térmica
//! genérica no entiende UTF-8: entiende una *página de códigos* de un byte.
//! Si se le manda UTF-8 directo, "jamón" sale como "jamÃ³n".
//!
//! La tabla de abajo NO está escrita a mano: se generó de los mapas oficiales
//! de Unicode (`CP437.TXT` y `CP850.TXT` de unicode.org), así que los bytes
//! son correctos por construcción.
//!
//! Un dato que salió de esas tablas y que explica el diseño:
//!   * **CP437 no puede representar `Á Í Ó Ú`** (le faltan 43 caracteres del
//!     Latin-1). Es la página que traen por omisión las térmicas chinas.
//!   * **CP850 sí cubre todo el Latin-1**, incluidas las mayúsculas acentuadas.
//!
//! Por eso el perfil por omisión manda `ESC t 2` (CP850) al empezar el ticket.
//! Y cuando una página no tiene un carácter, se le quita el acento (`Á`→`A`)
//! en vez de mandar basura: mejor "ARROZ" que "ARROZ" con símbolos raros.

/// Página de códigos que usa la impresora para interpretar los bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pagina {
    /// PC437 (DOS Estados Unidos). La de fábrica de casi todas las genéricas.
    Cp437,
    /// PC850 (DOS Europa Occidental). Cubre todo el español, inclusive Á Í Ó Ú.
    #[default]
    Cp850,
    /// Windows-1252. La usan algunas Epson y las impresoras en modo "Windows".
    Cp1252,
    /// Pasa el UTF-8 tal cual. Sólo para impresoras modernas (o para mandar
    /// bytes a un archivo y revisarlos a mano).
    Utf8,
}

impl Pagina {
    /// Byte que se manda en `ESC t n` para seleccionar esta página.
    /// `None` = la impresora no acepta el comando (se deja la de fábrica).
    ///
    /// Los valores son los del estándar ESC/POS: 0=CP437, 2=CP850, 16=WPC1252.
    pub fn selector(self) -> Option<u8> {
        match self {
            Pagina::Cp437 => Some(0),
            Pagina::Cp850 => Some(2),
            Pagina::Cp1252 => Some(16),
            Pagina::Utf8 => None,
        }
    }

    /// Nombre corto, para guardarlo como texto en la configuración.
    pub fn texto(self) -> &'static str {
        match self {
            Pagina::Cp437 => "cp437",
            Pagina::Cp850 => "cp850",
            Pagina::Cp1252 => "cp1252",
            Pagina::Utf8 => "utf8",
        }
    }

    /// Lo inverso de [`Pagina::texto`]. Lo desconocido cae en CP850.
    pub fn desde_texto(s: &str) -> Pagina {
        match s.trim().to_ascii_lowercase().as_str() {
            "cp437" | "437" | "pc437" => Pagina::Cp437,
            "cp850" | "850" | "pc850" => Pagina::Cp850,
            "cp1252" | "1252" | "windows-1252" => Pagina::Cp1252,
            "utf8" | "utf-8" => Pagina::Utf8,
            _ => Pagina::Cp850,
        }
    }

    /// Nota honesta para la pantalla de configuración.
    pub fn nota(self) -> &'static str {
        match self {
            Pagina::Cp437 => {
                "La de fábrica en las genéricas. No trae Á, Í, Ó, Ú: se imprimen sin acento."
            }
            Pagina::Cp850 => "Recomendada: español completo (ñ, acentos, ¿¡).",
            Pagina::Cp1252 => "Windows: para Epson y similares en modo Windows.",
            Pagina::Utf8 => "Sólo impresoras modernas. Si salen caracteres raros, usa CP850.",
        }
    }
}

/// Latin-1 (U+00A0..U+00FF) → byte en CP437. `0x00` = no representable.
/// Generada de unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT
static CP437_LATIN1: [u8; 96] = [
    0xFF, 0xAD, 0x9B, 0x9C, 0x00, 0x9D, 0x00, 0x00, 0x00, 0x00, 0xA6, 0xAE, 0xAA, 0x00, 0x00, 0x00,
    0xF8, 0xF1, 0xFD, 0x00, 0x00, 0xE6, 0x00, 0xFA, 0x00, 0x00, 0xA7, 0xAF, 0xAC, 0xAB, 0x00, 0xA8,
    0x00, 0x00, 0x00, 0x00, 0x8E, 0x8F, 0x92, 0x80, 0x00, 0x90, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0xA5, 0x00, 0x00, 0x00, 0x00, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9A, 0x00, 0x00, 0xE1,
    0x85, 0xA0, 0x83, 0x00, 0x84, 0x86, 0x91, 0x87, 0x8A, 0x82, 0x88, 0x89, 0x8D, 0xA1, 0x8C, 0x8B,
    0x00, 0xA4, 0x95, 0xA2, 0x93, 0x00, 0x94, 0xF6, 0x00, 0x97, 0xA3, 0x96, 0x81, 0x00, 0x00, 0x98,
];

/// Latin-1 (U+00A0..U+00FF) → byte en CP850. `0x00` = no representable
/// (no hay ninguno: CP850 cubre el Latin-1 completo).
/// Generada de unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP850.TXT
static CP850_LATIN1: [u8; 96] = [
    0xFF, 0xAD, 0xBD, 0x9C, 0xCF, 0xBE, 0xDD, 0xF5, 0xF9, 0xB8, 0xA6, 0xAE, 0xAA, 0xF0, 0xA9, 0xEE,
    0xF8, 0xF1, 0xFD, 0xFC, 0xEF, 0xE6, 0xF4, 0xFA, 0xF7, 0xFB, 0xA7, 0xAF, 0xAC, 0xAB, 0xF3, 0xA8,
    0xB7, 0xB5, 0xB6, 0xC7, 0x8E, 0x8F, 0x92, 0x80, 0xD4, 0x90, 0xD2, 0xD3, 0xDE, 0xD6, 0xD7, 0xD8,
    0xD1, 0xA5, 0xE3, 0xE0, 0xE2, 0xE5, 0x99, 0x9E, 0x9D, 0xEB, 0xE9, 0xEA, 0x9A, 0xED, 0xE8, 0xE1,
    0x85, 0xA0, 0x83, 0xC6, 0x84, 0x86, 0x91, 0x87, 0x8A, 0x82, 0x88, 0x89, 0x8D, 0xA1, 0x8C, 0x8B,
    0xD0, 0xA4, 0x95, 0xA2, 0x93, 0xE4, 0x94, 0xF6, 0x9B, 0x97, 0xA3, 0x96, 0x81, 0xEC, 0xE7, 0x98,
];

/// Quita el acento de una letra latina. `None` = no se puede reducir a una letra.
fn sin_acento(c: char) -> Option<char> {
    let r = match c {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' => 'A',
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' => 'E',
        'è' | 'é' | 'ê' | 'ë' | 'ē' => 'e',
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' => 'I',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' => 'O',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' => 'U',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'Ñ' => 'N',
        'ñ' => 'n',
        'Ç' => 'C',
        'ç' => 'c',
        'Ý' | 'Ÿ' => 'Y',
        'ý' | 'ÿ' => 'y',
        'Ð' => 'D',
        'ð' => 'd',
        'Þ' => 'T',
        'ß' => 's',
        'Æ' => 'A',
        'æ' => 'a',
        _ => return None,
    };
    Some(r)
}

/// Caracteres Unicode frecuentes que no están en el Latin-1 pero que se pueden
/// "achatar" sin perder el sentido (comillas curly, rayas, puntos suspensivos).
fn achatar(c: char) -> Option<&'static str> {
    match c {
        '\u{2013}' | '\u{2014}' | '\u{2212}' => Some("-"),
        '\u{2018}' | '\u{2019}' | '\u{00B4}' => Some("'"),
        '\u{201C}' | '\u{201D}' | '\u{00AB}' | '\u{00BB}' => Some("\""),
        '\u{2026}' => Some("..."),
        '\u{00A0}' => Some(" "),
        '\u{20AC}' => Some("EUR"),
        '\u{2192}' | '\u{279C}' => Some("->"),
        _ => None,
    }
}

/// Convierte texto UTF-8 (español) a los bytes que la impresora espera.
///
/// Reglas, en orden:
///   1. ASCII imprimible, tabulador y salto de línea: pasan igual.
///   2. Latin-1 (`U+00A0..U+00FF`): se busca en la tabla de la página.
///   3. Si la página no lo tiene: se le quita el acento (`Á`→`A`).
///   4. Si aun así no se puede: se achata (`…`→`...`) o se pone `?`.
///
/// Nunca devuelve un byte de control fuera de `\n` y `\t`: así un carácter
/// raro no puede "comerse" el resto del ticket.
pub fn transcribir(texto: &str, pagina: Pagina) -> Vec<u8> {
    let mut out = Vec::with_capacity(texto.len() + 8);
    // Los saltos de línea de Windows (CRLF) se colapsan antes de transcribir:
    // si no, cada salto imprimiría dos líneas en la térmica.
    let plano: std::borrow::Cow<str> = if texto.contains('\r') {
        std::borrow::Cow::Owned(texto.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(texto)
    };
    for ch in plano.chars() {
        // 1. ASCII
        if ch.is_ascii() {
            let b = ch as u8;
            if b == b'\n' || b == b'\t' || (0x20..0x7F).contains(&b) {
                out.push(b);
            } else if b != 0 {
                // Control (incluido \r): se sustituye para no romper el ticket.
                out.push(if b == b'\r' { b'\n' } else { b'?' });
            }
            continue;
        }

        if pagina == Pagina::Utf8 {
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            continue;
        }

        // 2. Latin-1: tabla de la página (CP1252 es el Latin-1 tal cual).
        let u = ch as u32;
        if let Some(b) = byte_latin1(u, pagina) {
            out.push(b);
            continue;
        }

        // 3. Sin acento
        if let Some(plano) = sin_acento(ch) {
            out.push(plano as u8);
            continue;
        }

        // 4. Achatar o rendirse
        match achatar(ch) {
            Some(s) => out.extend_from_slice(s.as_bytes()),
            None => out.push(b'?'),
        }
    }
    out
}

/// Busca un carácter del rango U+00A0..U+00FF en la tabla de la página.
fn byte_latin1(u: u32, pagina: Pagina) -> Option<u8> {
    if !(0xA0..=0xFF).contains(&u) {
        return None;
    }
    let idx = (u - 0xA0) as usize;
    let b = match pagina {
        Pagina::Cp437 => CP437_LATIN1[idx],
        Pagina::Cp850 => CP850_LATIN1[idx],
        // WPC1252 = Latin-1 en este rango (sólo difiere en 0x80..0x9F).
        Pagina::Cp1252 => idx as u8 + 0xA0,
        Pagina::Utf8 => unreachable!(),
    };
    if b == 0 {
        None
    } else {
        Some(b)
    }
}

/// Al revés: bytes de la impresora → texto legible.
///
/// Sirve para la vista previa en pantalla. Parece un detalle, pero no lo es:
/// con `String::from_utf8_lossy` los acentos salían como basura justo en la
/// pantalla donde el cajero revisa el ticket antes de cobrar.
pub fn decodificar(bytes: &[u8], pagina: Pagina) -> String {
    if pagina == Pagina::Utf8 {
        return String::from_utf8_lossy(bytes).to_string();
    }
    let inversa = inversa_de(pagina);
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x80 {
            out.push(b as char);
        } else {
            match inversa[b as usize] {
                0 => out.push('·'), // carácter que la página no define
                u => out.push(char::from_u32(u).unwrap_or('·')),
            }
        }
    }
    out
}

/// Tabla byte → punto de código, construida al vuelo desde la tabla directa.
fn inversa_de(pagina: Pagina) -> [u32; 256] {
    let mut inv = [0u32; 256];
    let directa = match pagina {
        Pagina::Cp437 => CP437_LATIN1,
        Pagina::Cp850 | Pagina::Cp1252 => CP850_LATIN1,
        Pagina::Utf8 => unreachable!(),
    };
    for (i, &b) in directa.iter().enumerate() {
        let u = 0xA0 + i as u32;
        if pagina == Pagina::Cp1252 {
            // WPC1252 es el Latin-1 tal cual en este rango.
            inv[0xA0 + i] = u;
            continue;
        }
        if b != 0 {
            inv[b as usize] = u;
        }
    }
    if pagina == Pagina::Cp1252 {
        return inv;
    }
    // Los caracteres de dibujo y símbolos de las páginas DOS se muestran como
    // algo parecido: la vista previa no tiene por qué ser exacta, pero no debe
    // engañar. Los bloques y líneas se ven como '-', '|' y '+'.
    for b in 0xB0u16..=0xDF {
        if inv[b as usize] == 0 {
            inv[b as usize] = '-' as u32;
        }
    }
    inv
}

/// Compatibilidad: CP437 como se hacía antes.
///
/// Se conserva porque había tickets y pruebas construidos con ella, pero el
/// camino bueno es [`transcribir`] con el [`Perfil`](crate::perfil::Perfil)
/// de la impresora: CP850 tiene las mayúsculas acentuadas, CP437 no.
pub fn to_cp437(texto: &str) -> Vec<u8> {
    transcribir(texto, Pagina::Cp437)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_espanol_completo_cabe_en_cp850() {
        let texto = "á é í ó ú ñ Ñ Á É Í Ó Ú ¿ ¡ ü Ü";
        let bytes = transcribir(texto, Pagina::Cp850);
        // Ni un solo carácter perdido: todo se resolvió a un byte de la página.
        assert!(!bytes.contains(&b'?'), "se perdió un carácter: {texto}");
        assert_eq!(bytes.len(), texto.chars().count());
    }

    #[test]
    fn cp437_no_tiene_mayusculas_acentuadas_pero_no_rompe() {
        // CP437 no puede con la Ó: la imprime sin acento antes que basura.
        let bytes = transcribir("JABÓN", Pagina::Cp437);
        assert_eq!(bytes, b"JABON");
        // Las minúsculas sí están tal cual (0xA2 = ó en CP437).
        assert_eq!(transcribir("jamón", Pagina::Cp437), b"jam\xA2n");
    }

    #[test]
    fn cp437_y_cp850_coinciden_en_las_minusculas_del_espanol() {
        for texto in ["ñ", "á", "é", "í", "ó", "ú", "¿", "¡"] {
            assert_eq!(
                transcribir(texto, Pagina::Cp437),
                transcribir(texto, Pagina::Cp850),
                "difieren en {texto}"
            );
        }
    }

    #[test]
    fn las_tablas_salen_de_los_mapas_oficiales_de_unicode() {
        // Puntos de control contra las tablas de unicode.org.
        assert_eq!(transcribir("ñ", Pagina::Cp437), vec![0xA4]);
        assert_eq!(transcribir("Ñ", Pagina::Cp850), vec![0xA5]);
        assert_eq!(transcribir("¿", Pagina::Cp850), vec![0xA8]);
        assert_eq!(transcribir("¡", Pagina::Cp850), vec![0xAD]);
        assert_eq!(transcribir("é", Pagina::Cp850), vec![0x82]);
        assert_eq!(transcribir("É", Pagina::Cp850), vec![0x90]);
        // Espacio duro (U+00A0) = 0xFF en las dos páginas DOS.
        assert_eq!(transcribir("\u{00A0}", Pagina::Cp850), vec![0xFF]);
    }

    #[test]
    fn cp1252_es_latin1_tal_cual() {
        // WPC1252 = Latin-1: ó es el byte 0xF3, no los dos bytes de UTF-8.
        assert_eq!(transcribir("jamón", Pagina::Cp1252), b"jam\xF3n");
        assert_eq!(transcribir("Ñ", Pagina::Cp1252), vec![0xD1]);
    }

    #[test]
    fn utf8_pasa_directo() {
        assert_eq!(transcribir("jamón", Pagina::Utf8), "jamón".as_bytes());
    }

    #[test]
    fn los_caracteres_raros_nunca_rompen_el_ticket() {
        // Un emoji, una comilla curly y puntos suspensivos.
        let bytes = transcribir("café ☕ “express”…", Pagina::Cp850);
        assert!(!bytes.contains(&b'\n'), "se coló un salto de línea");
        assert!(
            !bytes
                .iter()
                .any(|b| *b < 0x20 && *b != b'\n' && *b != b'\t'),
            "se coló un byte de control"
        );
        let texto = String::from_utf8_lossy(&bytes);
        assert!(texto.contains("caf") && bytes.windows(2).any(|w| w == [0x82, b' ']));
        assert!(texto.contains("express"));
        assert!(texto.contains("..."));
    }

    #[test]
    fn el_caracter_de_control_no_se_come_el_ticket() {
        let bytes = transcribir("hola\u{0007}mundo\u{0000}", Pagina::Cp850);
        assert_eq!(bytes, b"hola?mundo");
    }

    #[test]
    fn el_retorno_de_carros_se_vuelve_salto_de_linea() {
        // Un CSV con saltos de Windows no debe atorar a la impresora.
        assert_eq!(transcribir("a\r\nb", Pagina::Cp850), b"a\nb");
    }

    #[test]
    fn las_paginas_viajan_por_texto_y_vuelven() {
        for p in [Pagina::Cp437, Pagina::Cp850, Pagina::Cp1252, Pagina::Utf8] {
            assert_eq!(Pagina::desde_texto(p.texto()), p);
        }
        // Lo desconocido cae en la recomendada, no en error.
        assert_eq!(Pagina::desde_texto("una tontería"), Pagina::Cp850);
        assert_eq!(Pagina::default(), Pagina::Cp850);
    }

    #[test]
    fn el_selector_de_pagina_es_el_estandar_escpos() {
        assert_eq!(Pagina::Cp437.selector(), Some(0));
        assert_eq!(Pagina::Cp850.selector(), Some(2));
        assert_eq!(Pagina::Cp1252.selector(), Some(16));
        // UTF-8 no se selecciona con ESC t: se deja la página de fábrica.
        assert_eq!(Pagina::Utf8.selector(), None);
    }

    #[test]
    fn el_texto_vuelve_igual_despues_de_un_round_trip() {
        for pagina in [Pagina::Cp437, Pagina::Cp850, Pagina::Cp1252] {
            for texto in ["Abarrotes Lupita", "jamón", "ÑANDÚ", "¿qué tal?", "¡Hola!"] {
                let bytes = transcribir(texto, pagina);
                let vuelto = decodificar(&bytes, pagina);
                // CP437 no tiene mayúsculas acentuadas: se acepta la versión
                // sin acento, nunca un '?' ni basura.
                let esperado: String = texto
                    .chars()
                    .map(|c| match (pagina, c) {
                        (Pagina::Cp437, 'Á') => 'A',
                        (Pagina::Cp437, 'Ú') => 'U',
                        (_, c) => c,
                    })
                    .collect();
                assert_eq!(vuelto, esperado, "round trip en {pagina:?}: {texto}");
            }
        }
    }

    #[test]
    fn decodificar_sirve_para_la_vista_previa() {
        let bytes = transcribir("Ticket de ÑANDÚ: $12.50", Pagina::Cp850);
        let previo = decodificar(&bytes, Pagina::Cp850);
        assert!(
            previo.contains("ÑANDÚ"),
            "la vista previa se rompió: {previo}"
        );
        assert!(previo.contains("$12.50"));
    }

    #[test]
    fn to_cp437_se_mantiene_compatible() {
        assert_eq!(to_cp437("ñ"), vec![0xA4]);
        assert_eq!(to_cp437("jamón"), vec![b'j', b'a', b'm', 0xA2, b'n']);
        assert_eq!(to_cp437("¿qué?"), vec![0xA8, b'q', b'u', 0x82, b'?']);
    }
}
