//! Códigos EAN-13 internos (granel, ropa, lo que no trae código de fábrica).
//!
//! Prefijo **20**: es el rango de uso interno en tienda (GS1). El dígito de
//! control se calcula igual que en un EAN de fábrica, para que el lector no
//! distinga uno de otro.

/// Dígito de control de un EAN-13 a partir de los 12 dígitos de datos.
pub fn digito_control(doce: &str) -> Option<u8> {
    if doce.len() != 12 || !doce.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut suma: u32 = 0;
    for (i, b) in doce.bytes().enumerate() {
        let d = (b - b'0') as u32;
        suma += if i % 2 == 0 { d } else { d * 3 };
    }
    Some(((10 - (suma % 10)) % 10) as u8)
}

/// Completa 12 dígitos con el de control. `None` si no son 12 números.
pub fn ean13_de(doce: &str) -> Option<String> {
    let d = digito_control(doce)?;
    Some(format!("{doce}{d}"))
}

/// ¿Es un EAN-13 de 13 dígitos con el control bien puesto?
pub fn ean13_valido(codigo: &str) -> bool {
    let d: String = codigo.chars().filter(|c| c.is_ascii_digit()).collect();
    if d.len() != 13 {
        return false;
    }
    ean13_de(&d[..12]).as_deref() == Some(d.as_str())
}

/// Prefijo de uso interno en tienda (GS1 20–29).
pub const PREFIJO_INTERNO: &str = "20";

/// Arma un EAN-13 interno: `20` + 10 dígitos de correlativo + control.
pub fn ean13_interno(n: u64) -> Option<String> {
    if n == 0 || n > 9_999_999_999 {
        return None;
    }
    ean13_de(&format!("20{n:010}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_digito_de_control_es_el_del_estandar() {
        // Ejemplo público: 400638133393 + 1.
        assert_eq!(digito_control("400638133393"), Some(1));
        assert_eq!(ean13_de("400638133393").as_deref(), Some("4006381333931"));
        assert!(ean13_valido("4006381333931"));
        assert!(!ean13_valido("4006381333932"));
        assert!(!ean13_valido("123"));
    }

    #[test]
    fn el_interno_empieza_en_20_y_el_lector_lo_acepta() {
        let c = ean13_interno(1).unwrap();
        assert!(c.starts_with("20"));
        assert_eq!(c.len(), 13);
        assert!(ean13_valido(&c));
        assert_eq!(c, "2000000000015");
    }
}
