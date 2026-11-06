//! Solicitud de cancelación CFDI 4.0 (catálogo `c_MotivoCancelacion`).
//!
//! El SAT no se habla por HTTP desde aquí: se deja el XML en la carpeta para
//! que el contador lo presente. Sin UUID (la factura no se había timbrado)
//! no hay nada que cancelar ante el SAT; eso se resuelve en el programa.

/// Catálogo `c_MotivoCancelacion` (SAT, 2022 en adelante).
pub const MOTIVOS: [(&str, &str); 4] = [
    (
        "01",
        "Errores con relación (lleva la factura que sustituye)",
    ),
    ("02", "Errores sin relación"),
    ("03", "No se llevó a cabo la operación"),
    ("04", "Nominativa relacionada en una factura global"),
];

/// ¿Es un UUID del SAT (8-4-4-4-12 hexadecimales)?
pub fn uuid_sat(s: &str) -> bool {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    let hex = |c: u8| c.is_ascii_hexdigit();
    let guion = |i: usize| b[i] == b'-';
    for (i, &c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if !guion(i) {
                    return false;
                }
            }
            _ => {
                if !hex(c) {
                    return false;
                }
            }
        }
    }
    true
}

/// Revisa motivo y, si es 01, el UUID de la factura que sustituye.
pub fn validar(motivo: &str, uuid_sustitucion: &str) -> Result<(), String> {
    let motivo = motivo.trim();
    if !MOTIVOS.iter().any(|(c, _)| *c == motivo) {
        return Err("El motivo de cancelación es 01, 02, 03 o 04 (catálogo del SAT).".into());
    }
    if motivo == "01" {
        if !uuid_sat(uuid_sustitucion) {
            return Err("El motivo 01 lleva el UUID de la factura que sustituye a ésta.".into());
        }
    } else if !uuid_sustitucion.trim().is_empty() {
        return Err(
            "El UUID de sustitución sólo se usa con el motivo 01 (errores con relación).".into(),
        );
    }
    Ok(())
}

/// XML de solicitud de cancelación. El contador lo presenta al SAT.
pub fn xml(
    rfc_emisor: &str,
    fecha_iso: &str,
    uuid: &str,
    motivo: &str,
    uuid_sustitucion: &str,
) -> Result<String, String> {
    validar(motivo, uuid_sustitucion)?;
    if !uuid_sat(uuid) {
        return Err("Esa factura no trae UUID: no se puede cancelar ante el SAT.".into());
    }
    let rfc = rfc_emisor.trim().to_uppercase();
    if rfc.len() != 12 && rfc.len() != 13 {
        return Err("El RFC del emisor no está completo.".into());
    }
    let uuid = uuid.trim().to_uppercase();
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<Cancelacion xmlns=\"http://cancelacfd.sat.gob.mx\"");
    s.push_str(&format!(" Fecha=\"{fecha_iso}\" RfcEmisor=\"{rfc}\">\n"));
    s.push_str("  <Folios>\n");
    s.push_str(&format!("    <UUID>{uuid}</UUID>\n"));
    s.push_str(&format!("    <Motivo>{motivo}</Motivo>\n"));
    if motivo == "01" {
        let sust = uuid_sustitucion.trim().to_uppercase();
        s.push_str(&format!(
            "    <FolioSustitucion>{sust}</FolioSustitucion>\n"
        ));
    }
    s.push_str("  </Folios>\n");
    s.push_str("</Cancelacion>\n");
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_motivo_01_exige_la_factura_que_sustituye() {
        assert!(validar("01", "").is_err());
        assert!(validar("01", "AAAA1111-2222-3333-4444-555566667777").is_ok());
        assert!(validar("02", "").is_ok());
        assert!(validar("02", "AAAA1111-2222-3333-4444-555566667777").is_err());
        assert!(validar("05", "").is_err());
    }

    #[test]
    fn el_xml_lleva_motivo_y_uuid() {
        let x = xml(
            "XAXX010101000",
            "2026-10-12T12:00:00",
            "a1b2c3d4-1111-2222-3333-444455556666",
            "02",
            "",
        )
        .unwrap();
        assert!(x.contains("RfcEmisor=\"XAXX010101000\""));
        assert!(x.contains("<UUID>A1B2C3D4-1111-2222-3333-444455556666</UUID>"));
        assert!(x.contains("<Motivo>02</Motivo>"));
        assert!(!x.contains("FolioSustitucion"));
    }

    #[test]
    fn el_xml_del_01_lleva_la_sustitucion() {
        let x = xml(
            "XAXX010101000",
            "2026-10-12T12:00:00",
            "AAAA1111-2222-3333-4444-555566667777",
            "01",
            "BBBB1111-2222-3333-4444-555566667777",
        )
        .unwrap();
        assert!(
            x.contains("<FolioSustitucion>BBBB1111-2222-3333-4444-555566667777</FolioSustitucion>")
        );
    }
}
