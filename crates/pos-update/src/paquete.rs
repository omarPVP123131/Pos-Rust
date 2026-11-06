//! Formato del paquete de actualización y su verificación.
//!
//! Un paquete `.posup` es un archivo de tres partes, sin compresión ni
//! dependencias raras: se puede leer con cualquier editor y se puede copiar en
//! una memoria USB.
//!
//! ```text
//! POSUP1\n
//! {"version":"0.5.0","min_version":"0.4.0", ... }\n   <- manifiesto, una línea
//! <bytes del ejecutable>                              <- el payload
//! ```
//!
//! La firma es Ed25519 sobre `"<sha256 del payload>|<version>"`. La llave
//! pública va **dentro del binario** (`llaves/pos-publica.txt`): aunque alguien
//! le entregue al encargado un archivo firmado por otro, el punto de venta lo
//! rechaza. La llave privada nunca sale de la máquina donde se firma.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{a_hex, de_hex, Error, Result};

/// Versión del formato. Si el día de mañana cambia, se lee aquí y no explota.
pub const MAGIA: &str = "POSUP1";

/// Qué describe el manifiesto. Va en JSON para poder inspeccionarlo a mano.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Manifiesto {
    /// Versión que se instala, por ejemplo `"0.5.0"`.
    pub version: String,
    /// Desde qué versión se puede subir a ésta. Evita saltos que rompan la base.
    pub min_version: String,
    /// Lo que se le muestra al encargado antes de aceptar.
    pub notas: String,
    /// Tamaño del ejecutable, en bytes.
    pub tamanio: u64,
    /// SHA-256 del ejecutable, en hexadecimal.
    pub sha256: String,
    /// Firma Ed25519 en hexadecimal, sobre `"<sha256>|<version>"`.
    pub firma: String,
}

/// Un paquete ya leído y separado. Todavía no se ha verificado.
#[derive(Clone, Debug)]
pub struct Paquete {
    pub manifiesto: Manifiesto,
    /// Los bytes del ejecutable nuevo.
    pub payload: Vec<u8>,
}

impl Paquete {
    pub fn version(&self) -> &str {
        &self.manifiesto.version
    }
}

/// Parte el archivo en la línea del manifiesto y el payload.
///
/// El payload es un ejecutable: puede contener saltos de línea, así que no se
/// puede leer "línea por línea". Por eso el manifiesto va en una sola línea y
/// el resto del archivo es el binario, sin contar tamaños por ningún lado.
pub(crate) fn partir(bytes: &[u8]) -> Result<(&str, &[u8])> {
    if bytes.len() <= MAGIA.len() || bytes[MAGIA.len()] != b'\n' {
        return Err(Error::Formato(
            "El paquete no traía manifiesto (está incompleto)".into(),
        ));
    }
    let resto = &bytes[MAGIA.len() + 1..];
    let fin = match resto.iter().position(|b| *b == b'\n') {
        Some(i) => i,
        None => {
            return Err(Error::Formato(
                "El paquete no traía manifiesto (está incompleto)".into(),
            ))
        }
    };
    let linea = std::str::from_utf8(&resto[..fin])
        .map_err(|_| Error::Formato("El manifiesto del paquete no es texto válido".into()))?;
    Ok((linea, &resto[fin + 1..]))
}

/// Lee y verifica un paquete: firma, hash y que la versión sí suba.
///
/// Este es el único camino por el que un archivo se convierte en actualización.
/// Si algo falla, el error dice qué pasó en las palabras del encargado.
pub fn revisar(bytes: &[u8], version_actual: &str) -> Result<Paquete> {
    let paquete = leer(bytes)?;
    verificar(&paquete, version_actual)?;
    Ok(paquete)
}

/// Separa el archivo en manifiesto y payload, y comprueba la firma y el hash
/// con la llave que trae el binario.
pub fn leer(bytes: &[u8]) -> Result<Paquete> {
    let publica = crate::llave_publica()?;
    leer_con(bytes, &publica)
}

/// Igual que [`leer`], pero contra una llave concreta.
///
/// Existe para que las pruebas puedan firmar con otra llave y comprobar que el
/// filtro es la firma, sin duplicar la lógica: las pruebas ejercen el mismo
/// código que la tienda.
pub fn leer_con(bytes: &[u8], publica: &[u8; 32]) -> Result<Paquete> {
    let texto = bytes.get(..MAGIA.len()).unwrap_or_default();
    if texto != MAGIA.as_bytes() {
        return Err(Error::Formato(
            "Este archivo no es un paquete de actualización".into(),
        ));
    }
    let (linea, payload) = partir(bytes)?;
    let manifiesto: Manifiesto = serde_json::from_str(linea)
        .map_err(|e| Error::Formato(format!("No entiendo el manifiesto: {e}")))?;

    if payload.len() as u64 != manifiesto.tamanio {
        return Err(Error::Formato(format!(
            "El paquete dice traer {} bytes y trae {} (está truncado)",
            manifiesto.tamanio,
            payload.len()
        )));
    }

    // 1) El ejecutable es exactamente el que se firmó.
    let calculado = a_hex(&Sha256::digest(payload));
    if calculado != manifiesto.sha256 {
        return Err(Error::HashIncorrecto);
    }

    // 2) Y alguien con la llave privada lo firmó.
    let firma_bytes: [u8; 64] = de_hex(&manifiesto.firma)
        .and_then(|v| v.try_into().ok())
        .ok_or(Error::FirmaInvalida)?;
    let firma = Signature::from_bytes(&firma_bytes);
    let verificador = VerifyingKey::from_bytes(publica).map_err(|_| Error::LlaveMala)?;
    verificador
        .verify(
            material_firmado(&manifiesto.sha256, &manifiesto.version).as_bytes(),
            &firma,
        )
        .map_err(|_| Error::FirmaInvalida)?;

    Ok(Paquete {
        manifiesto,
        payload: payload.to_vec(),
    })
}

/// Comprueba que la versión sí sea un avance y que no se salte ninguna base.
pub fn verificar(paquete: &Paquete, version_actual: &str) -> Result<()> {
    let nueva = partes(&paquete.manifiesto.version);
    let actual = partes(version_actual);
    if nueva <= actual {
        return Err(Error::VersionVieja {
            actual: version_actual.to_string(),
            paquete: paquete.manifiesto.version.clone(),
        });
    }
    let minima = partes(&paquete.manifiesto.min_version);
    if actual < minima {
        return Err(Error::MuyNuevo {
            actual: version_actual.to_string(),
            minimo: paquete.manifiesto.min_version.clone(),
        });
    }
    Ok(())
}

/// Arma un paquete firmado. La usa la herramienta que firma y las pruebas.
pub fn armar(
    version: &str,
    min_version: &str,
    notas: &str,
    payload: &[u8],
    privada: &[u8; 32],
) -> Result<Vec<u8>> {
    use ed25519_dalek::{Signer, SigningKey};
    let sha256 = a_hex(&Sha256::digest(payload));
    let firmante = SigningKey::from_bytes(privada);
    let firma = firmante.sign(material_firmado(&sha256, version).as_bytes());
    let manifiesto = Manifiesto {
        version: version.to_string(),
        min_version: min_version.to_string(),
        notas: notas.to_string(),
        tamanio: payload.len() as u64,
        sha256,
        firma: a_hex(&firma.to_bytes()),
    };
    let linea = serde_json::to_string(&manifiesto)
        .map_err(|e| Error::Formato(format!("No pude armar el manifiesto: {e}")))?;
    let mut salida = Vec::new();
    salida.extend_from_slice(MAGIA.as_bytes());
    salida.push(b'\n');
    salida.extend_from_slice(linea.as_bytes());
    salida.push(b'\n');
    salida.extend_from_slice(payload);
    Ok(salida)
}

/// Lo que se firma: el contenido y la versión juntos. Si se firmara sólo el
/// hash, un paquete viejo firmado se podría reetiquetar como nuevo.
pub fn material_firmado(sha256: &str, version: &str) -> String {
    format!("{sha256}|{version}")
}

/// `"0.10.3"` -> `(0, 10, 3)`. Lo que no sea número cuenta como cero.
pub fn partes(version: &str) -> (u32, u32, u32) {
    let mut n = [0u32; 3];
    for (i, trozo) in version.split('.').take(3).enumerate() {
        let digitos: String = trozo.chars().take_while(|c| c.is_ascii_digit()).collect();
        n[i] = digitos.parse().unwrap_or(0);
    }
    (n[0], n[1], n[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La llave con la que firman las pruebas: es la **de desarrollo**, la
    /// misma que trae el binario, así que se prueba el camino completo.
    fn privada() -> [u8; 32] {
        crate::llave_privada_de_desarrollo().expect("llaves/pos-privada.txt")
    }

    /// La pública que corresponde a la llave de desarrollo.
    fn publica() -> [u8; 32] {
        ed25519_dalek::SigningKey::from_bytes(&privada())
            .verifying_key()
            .to_bytes()
    }

    /// Otra llave: sirve para probar que lo que no firmamos no entra.
    fn publica_de_otro() -> [u8; 32] {
        ed25519_dalek::SigningKey::from_bytes(&[7u8; 32])
            .verifying_key()
            .to_bytes()
    }

    fn paquete(version: &str, min: &str, payload: &[u8]) -> Vec<u8> {
        armar(version, min, "notas de prueba", payload, &privada()).unwrap()
    }

    #[test]
    fn un_paquete_bien_firmado_se_abre_y_dice_que_trae() {
        let bytes = paquete("0.5.0", "0.4.0", b"#!/bin/sh\necho hola\n");
        let p = leer_con(&bytes, &publica()).unwrap();
        assert_eq!(p.version(), "0.5.0");
        assert_eq!(p.payload, b"#!/bin/sh\necho hola\n".to_vec());
        assert_eq!(p.manifiesto.tamanio, 20);
        assert_eq!(p.manifiesto.notas, "notas de prueba");
    }

    #[test]
    fn firmar_y_verificar_de_ida_y_vuelta() {
        let payload = vec![1u8, 2, 3, 4, 5];
        let bytes = armar("1.2.3", "1.0.0", "ida y vuelta", &payload, &privada()).unwrap();
        let p = leer_con(&bytes, &publica()).unwrap();
        assert_eq!(p.payload, payload);
        assert_eq!(p.manifiesto.sha256, a_hex(&Sha256::digest(&payload)));
        // Y con la llave que trae el programa compilado, también.
        assert!(leer(&bytes).is_ok());
    }

    #[test]
    fn si_le_mueven_un_byte_al_ejecutable_ya_no_pasa() {
        let mut bueno = paquete("0.5.0", "0.4.0", b"ejecutable original");
        // El manifiesto se queda igual: sólo cambia el binario.
        let pos = bueno.len() - 1;
        bueno[pos] ^= 0xff;
        assert_eq!(
            leer_con(&bueno, &publica()).unwrap_err(),
            Error::HashIncorrecto
        );
    }

    #[test]
    fn un_paquete_firmado_con_otra_llave_no_entra() {
        let bytes = paquete("0.5.0", "0.4.0", b"ejecutable");
        assert_eq!(
            leer_con(&bytes, &publica_de_otro()).unwrap_err(),
            Error::FirmaInvalida
        );
    }

    #[test]
    fn cambiarle_la_version_a_un_paquete_viejo_lo_invalida() {
        // Firmado para la 0.5.0 y reetiquetado como 0.9.0: la firma cubre la
        // versión, así que el truco no pasa.
        let bytes = paquete("0.5.0", "0.4.0", b"ejecutable");
        let texto = String::from_utf8(bytes).unwrap();
        let chocado = texto.replace("\"version\":\"0.5.0\"", "\"version\":\"0.9.0\"");
        assert_eq!(
            leer_con(chocado.as_bytes(), &publica()).unwrap_err(),
            Error::FirmaInvalida
        );
    }

    #[test]
    fn no_se_puede_bajar_de_version() {
        let p = leer_con(&paquete("0.3.0", "0.1.0", b"x"), &publica()).unwrap();
        assert_eq!(
            verificar(&p, "0.4.0").unwrap_err(),
            Error::VersionVieja {
                actual: "0.4.0".into(),
                paquete: "0.3.0".into()
            }
        );
        // Ni a la misma: "actualizar" a lo que ya tienes no es actualizar.
        let p = leer_con(&paquete("0.4.0", "0.1.0", b"x"), &publica()).unwrap();
        assert!(matches!(
            verificar(&p, "0.4.0").unwrap_err(),
            Error::VersionVieja { .. }
        ));
    }

    #[test]
    fn si_le_faltan_versiones_intermedias_lo_dice_claro() {
        let p = leer_con(&paquete("0.6.0", "0.5.0", b"x"), &publica()).unwrap();
        assert_eq!(
            verificar(&p, "0.4.0").unwrap_err(),
            Error::MuyNuevo {
                actual: "0.4.0".into(),
                minimo: "0.5.0".into()
            }
        );
    }

    #[test]
    fn un_archivo_que_no_es_paquete_se_rechaza_sin_romper_nada() {
        assert!(matches!(
            revisar(b"hola mundo", "0.4.0").unwrap_err(),
            Error::Formato(_)
        ));
        assert!(matches!(
            revisar(b"", "0.4.0").unwrap_err(),
            Error::Formato(_)
        ));
        // La magia sí, pero nada más.
        assert!(matches!(
            revisar(MAGIA.as_bytes(), "0.4.0").unwrap_err(),
            Error::Formato(_)
        ));
        // Magia y salto, pero sin manifiesto.
        assert!(matches!(
            revisar(b"POSUP1\n", "0.4.0").unwrap_err(),
            Error::Formato(_)
        ));
    }

    #[test]
    fn un_paquete_truncado_se_nota() {
        let bytes = paquete("0.5.0", "0.4.0", b"ejecutable de verdad");
        let cortado = &bytes[..bytes.len() - 5];
        assert!(
            matches!(
                leer_con(cortado, &publica()).unwrap_err(),
                Error::Formato(_)
            ),
            "el tamaño ya no cuadra"
        );
    }

    #[test]
    fn el_manifiesto_va_en_una_linea_y_el_binario_despues() {
        // El ejecutable trae saltos de línea: por eso no se puede leer por
        // líneas y el manifiesto se separa a mano.
        let payload = b"#!/bin/sh\necho uno\necho dos\n";
        let bytes = paquete("0.5.0", "0.4.0", payload);
        let p = leer_con(&bytes, &publica()).unwrap();
        assert_eq!(p.payload, payload.to_vec());
    }

    #[test]
    fn las_versiones_se_comparan_por_numero_no_por_texto() {
        // "0.10.0" > "0.9.0", aunque como texto no lo parezca.
        assert!(partes("0.10.0") > partes("0.9.0"));
        assert!(partes("1.0.0") > partes("0.99.99"));
        assert_eq!(partes("0.4"), (0, 4, 0));
        assert_eq!(partes("cualquier cosa"), (0, 0, 0));
        assert_eq!(partes("2.0.1-beta"), (2, 0, 1));
    }
}
