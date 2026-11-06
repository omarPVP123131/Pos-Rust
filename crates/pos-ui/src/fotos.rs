//! Miniaturas de los productos estrella (el catálogo de demostración).
//!
//! Van quemadas en el binario: 10 JPEG de 96×96 pesan unos 25 KB, caben en
//! una Celeron. El resto de SKUs sigue con el empaque pintado.
//! No es lógica de negocio: si no hay foto, la interfaz no se queja.

/// JPEG de 96 px para un SKU, si es uno de los de demostración.
pub fn jpeg(sku: &str) -> Option<&'static [u8]> {
    match sku {
        "7501" => Some(include_bytes!("../fotos/7501.jpg")),
        "7502" => Some(include_bytes!("../fotos/7502.jpg")),
        "7503" => Some(include_bytes!("../fotos/7503.jpg")),
        "7504" => Some(include_bytes!("../fotos/7504.jpg")),
        "7505" => Some(include_bytes!("../fotos/7505.jpg")),
        "7506" => Some(include_bytes!("../fotos/7506.jpg")),
        "7507" => Some(include_bytes!("../fotos/7507.jpg")),
        "7508" => Some(include_bytes!("../fotos/7508.jpg")),
        "7509" => Some(include_bytes!("../fotos/7509.jpg")),
        "7510" => Some(include_bytes!("../fotos/7510.jpg")),
        _ => None,
    }
}
