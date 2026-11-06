//! Cómo llegar al PAC (el que timbra de verdad).
//!
//! El SAT no recibe comprobantes directos: hay que pasar por un Proveedor
//! Autorizado de Certificación. Cada PAC tiene su API, su contrato y sus manías,
//! así que esto se modela como un **trait**: el programa habla con "el PAC" y
//! no con Facturapi, SW Sapien o Diverza en particular.
//!
//! Viene una implementación que no necesita internet: `PacCarpeta`. Se deja el
//! XML en una carpeta y de la otra se recoge el timbrado. Suena a poco, pero es
//! exactamente como trabajan muchos despachos contables: el contador recoge los
//! XML, los timbra con su propio sistema y devuelve los sellados. Y permite
//! probar todo el camino sin credenciales.
//!
//! El timbrado por HTTP está **bloqueado**. [`PacHttp`] existe para rechazar
//! cualquier `http://` o `https://` sin abrir socket ni mandar el XML a nadie.

use std::fs;
use std::path::PathBuf;

/// El timbre que devuelve el PAC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timbre {
    /// El UUID con el que el SAT identifica al comprobante.
    pub uuid: String,
    pub fecha: String,
    pub sello_sat: String,
    pub no_certificado_sat: String,
    /// El XML ya timbrado, tal cual lo devolvió el PAC.
    pub xml: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorPac {
    /// No hubo respuesta: se puede reintentar.
    SinRespuesta(String),
    /// El PAC rechazó el comprobante: reintentar no arregla nada.
    Rechazado(String),
    Io(String),
}

impl std::fmt::Display for ErrorPac {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorPac::SinRespuesta(s) => write!(f, "El PAC no respondió: {s}"),
            ErrorPac::Rechazado(s) => write!(f, "El PAC rechazó el comprobante: {s}"),
            ErrorPac::Io(s) => write!(f, "No se pudo hablar con el PAC: {s}"),
        }
    }
}

impl std::error::Error for ErrorPac {}

/// Lo que cualquier PAC tiene que saber hacer.
pub trait Pac {
    fn nombre(&self) -> &str;
    /// Manda el XML y devuelve el timbre. Si hay que reintentar, devuelve
    /// `SinRespuesta`; si el comprobante está mal, `Rechazado`.
    fn timbrar(&self, xml: &str) -> Result<Timbre, ErrorPac>;
}

/// PAC por carpetas: escribe los XML pendientes en una carpeta y recoge los
/// timbrados de otra. Sin internet, sin credenciales y auditable: los archivos
/// quedan ahí para que el contador los vea.
pub struct PacCarpeta {
    /// Donde se dejan los XML por timbrar.
    pub salida: PathBuf,
    /// Donde aparecen los XML ya timbrados.
    pub entrada: PathBuf,
}

impl PacCarpeta {
    pub fn new(salida: impl Into<PathBuf>, entrada: impl Into<PathBuf>) -> Self {
        PacCarpeta {
            salida: salida.into(),
            entrada: entrada.into(),
        }
    }

    /// Guarda el XML por timbrar.
    ///
    /// El nombre del archivo se saca **del propio comprobante** (su folio), no
    /// se pasa por fuera: así el archivo que se deja y el que se busca después
    /// siempre se llaman igual, aunque el programa se cierre en medio.
    pub fn dejar(&self, xml: &str) -> Result<PathBuf, ErrorPac> {
        let nombre = nombre_de(xml);
        fs::create_dir_all(&self.salida)
            .map_err(|e| ErrorPac::Io(format!("{}: {e}", self.salida.display())))?;
        let ruta = self.salida.join(format!("{nombre}.xml"));
        fs::write(&ruta, xml)
            .map_err(|e| ErrorPac::Io(format!("no pude escribir {}: {e}", ruta.display())))?;
        Ok(ruta)
    }
}

impl Pac for PacCarpeta {
    fn nombre(&self) -> &str {
        "carpeta (el contador timbra por fuera)"
    }

    fn timbrar(&self, xml: &str) -> Result<Timbre, ErrorPac> {
        // El nombre sale del contenido: el mismo comprobante siempre cae en el
        // mismo archivo, así que reintentar no lo duplica.
        let nombre = nombre_de(xml);
        self.dejar(xml)?;
        let ruta = self.entrada.join(format!("{nombre}.xml"));
        if !ruta.exists() {
            return Err(ErrorPac::SinRespuesta(format!(
                "todavía no aparece {} en {}",
                ruta.file_name().unwrap_or_default().to_string_lossy(),
                self.entrada.display()
            )));
        }
        let timbrado = fs::read_to_string(&ruta)
            .map_err(|e| ErrorPac::Io(format!("no pude leer {}: {e}", ruta.display())))?;
        let mut t = extraer_timbre(&timbrado).ok_or_else(|| {
            ErrorPac::Rechazado(format!(
                "el archivo {} no trae un TimbreFiscalDigital",
                ruta.display()
            ))
        })?;
        t.xml = timbrado;
        Ok(t)
    }
}

/// El nombre del archivo para un comprobante: su folio si se puede leer, si no
/// una huella del contenido.
///
/// Se usa el folio porque al contador le sirve encontrar "G20261011-01.xml" y
/// no "3f9a2c...". Y se cae a la huella porque un comprobante sin folio existe.
pub fn nombre_de(xml: &str) -> String {
    let folio = extraer_atributo(xml, "cfdi:Comprobante", "Folio")
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    folio.unwrap_or_else(|| nombre_estable(xml))
}

/// Lee un atributo de un nodo concreto del comprobante.
pub fn extraer_atributo(xml: &str, nodo: &str, atributo: &str) -> Option<String> {
    let i = xml.find(&format!("<{nodo}"))?;
    let fin = xml[i..].find('>')?;
    let cuerpo = &xml[i + 1..i + fin];
    let busca = format!("{atributo}=\"");
    let j = cuerpo.find(&busca)? + busca.len();
    let valor: String = cuerpo[j..].chars().take_while(|c| *c != '"').collect();
    Some(valor)
}

/// Un nombre corto y estable a partir del XML, para no duplicar archivos.
pub fn nombre_estable(xml: &str) -> String {
    // Dos pasadas con semillas distintas: 32 caracteres hexadecimales, que es
    // el tamaño de un UUID sin guiones. FNV modificado, sin dependencias.
    let mut bajo: u64 = 14_695_981_039_346_656_037;
    let mut alto: u64 = 0x9E37_79B9_7F4A_7C15;
    for (i, b) in xml.as_bytes().iter().enumerate() {
        bajo ^= *b as u64;
        bajo = bajo.wrapping_mul(1_099_511_628_211);
        alto ^= (*b as u64).wrapping_add(i as u64);
        alto = alto.rotate_left(13).wrapping_mul(0x2545_F491_4F6C_DD1D);
    }
    format!("{alto:016x}{bajo:016x}")
}

/// Saca el UUID del `TimbreFiscalDigital` de un XML timbrado.
///
/// Se hace a mano, sin un parser XML: el nodo es uno solo y siempre trae el
/// UUID. Traer una librería de XML entera para leer cuatro atributos es el tipo
/// de peso que este programa no se puede permitir.
pub fn extraer_timbre(xml: &str) -> Option<Timbre> {
    let (_, cola) = xml.split_once("<tfd:TimbreFiscalDigital")?;
    let nodo: String = cola.chars().take_while(|c| *c != '>').collect();
    Some(Timbre {
        uuid: atributo(&nodo, "UUID")?.to_uppercase(),
        fecha: atributo(&nodo, "FechaTimbrado")
            .unwrap_or_default()
            .to_string(),
        sello_sat: atributo(&nodo, "SelloSAT").unwrap_or_default().to_string(),
        no_certificado_sat: atributo(&nodo, "NoCertificadoSAT")
            .unwrap_or_default()
            .to_string(),
        xml: String::new(),
    })
}

/// Lee un atributo de un nodo XML (`Nombre="valor"`).
fn atributo<'a>(nodo: &'a str, nombre: &str) -> Option<&'a str> {
    let busca = format!("{nombre}=\"");
    let i = nodo.find(&busca)?;
    let resto = &nodo[i + busca.len()..];
    let fin = resto.find('"')?;
    Some(&resto[..fin])
}

/// Para las pruebas: un PAC que timbra solo, sin salir de la computadora.
pub struct PacDePrueba;

/// `true` si el texto es una URL de internet, no una carpeta.
pub fn parece_url_http(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    t.starts_with("http://") || t.starts_with("https://")
}

/// El mensaje que ve el cajero cuando alguien intenta timbrar por internet.
pub fn mensaje_http_bloqueado(destino: &str) -> String {
    let donde = destino.trim();
    if donde.is_empty() {
        "El timbrado por internet está bloqueado. Este programa sólo usa carpetas.".into()
    } else {
        format!(
            "El timbrado por internet está bloqueado. Este programa sólo usa carpetas (se intentó: {donde})."
        )
    }
}

/// Cliente HTTP del PAC: no manda nada. Existe para que un intento se rechace
/// en la cara y quede escrito, no para hablar con Facturapi ni con nadie.
pub struct PacHttp {
    pub destino: String,
}

impl PacHttp {
    pub fn new(destino: impl Into<String>) -> Self {
        PacHttp {
            destino: destino.into(),
        }
    }
}

impl Pac for PacHttp {
    fn nombre(&self) -> &str {
        "HTTP bloqueado"
    }

    fn timbrar(&self, _xml: &str) -> Result<Timbre, ErrorPac> {
        Err(ErrorPac::Rechazado(mensaje_http_bloqueado(&self.destino)))
    }
}

impl Pac for PacDePrueba {
    fn nombre(&self) -> &str {
        "de prueba (no timbra de verdad)"
    }

    fn timbrar(&self, xml: &str) -> Result<Timbre, ErrorPac> {
        let uuid = nombre_estable(xml);
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            &uuid[0..8],
            &uuid[8..12],
            &uuid[12..16],
            &uuid[16..20],
            &uuid[20..32]
        );
        Ok(Timbre {
            uuid,
            fecha: "2026-10-11T18:42:00".to_string(),
            sello_sat: "SELLO-DE-PRUEBA".to_string(),
            no_certificado_sat: "00001000000500000000".to_string(),
            xml: xml.replace(
                "</cfdi:Comprobante>",
                "<tfd:TimbreFiscalDigital UUID=\"X\"/></cfdi:Comprobante>",
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timbrado(uuid: &str) -> String {
        format!(
            "<cfdi:Comprobante Version=\"4.0\">\
             <cfdi:Complemento>\
             <tfd:TimbreFiscalDigital xmlns:tfd=\"http://www.sat.gob.mx/TimbreFiscalDigital\" \
             Version=\"1.1\" UUID=\"{uuid}\" FechaTimbrado=\"2026-10-11T18:42:00\" \
             SelloCFD=\"abc\" NoCertificadoSAT=\"00001000000500000000\" SelloSAT=\"xyz\"/>\
             </cfdi:Complemento></cfdi:Comprobante>"
        )
    }

    #[test]
    fn el_timbre_se_saca_del_xml_timbrado() {
        let t = extraer_timbre(&timbrado("a1b2c3d4-1111-2222-3333-444455556666")).unwrap();
        assert_eq!(t.uuid, "A1B2C3D4-1111-2222-3333-444455556666");
        assert_eq!(t.no_certificado_sat, "00001000000500000000");
        assert_eq!(t.fecha, "2026-10-11T18:42:00");
    }

    #[test]
    fn un_xml_sin_timbre_no_engana() {
        assert!(extraer_timbre("<cfdi:Comprobante Version=\"4.0\"></cfdi:Comprobante>").is_none());
    }

    #[test]
    fn el_pac_de_carpeta_deja_el_xml_y_recoge_el_timbrado() {
        let dir = std::env::temp_dir().join("pos-fiscal-pac");
        let _ = fs::remove_dir_all(&dir);
        let salida = dir.join("por-timbrar");
        let entrada = dir.join("timbrados");
        fs::create_dir_all(&entrada).unwrap();
        let pac = PacCarpeta::new(&salida, &entrada);

        let xml = "<cfdi:Comprobante>hola</cfdi:Comprobante>";

        // Sin el archivo de vuelta: dice "todavía no", que es reintentable.
        match pac.timbrar(xml) {
            Err(ErrorPac::SinRespuesta(m)) => assert!(m.contains("todavía no aparece"), "{m}"),
            otro => panic!("esperaba SinRespuesta: {otro:?}"),
        }
        // Y el XML quedó en la carpeta de salida, con nombre estable.
        let nombre = nombre_estable(xml);
        let dejado = salida.join(format!("{nombre}.xml"));
        assert!(dejado.exists(), "no dejó el XML por timbrar");
        assert_eq!(fs::read_to_string(&dejado).unwrap(), xml);

        // Cuando el contador (o el PAC) pone el timbrado, se recoge.
        fs::write(entrada.join(format!("{nombre}.xml")), timbrado("abc-123")).unwrap();
        let t = pac.timbrar(xml).unwrap();
        assert_eq!(t.uuid, "ABC-123");
        assert!(t.xml.contains("TimbreFiscalDigital"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn el_nombre_del_archivo_es_estable_y_unico() {
        let a = nombre_estable("<cfdi:Comprobante>uno</cfdi:Comprobante>");
        let b = nombre_estable("<cfdi:Comprobante>uno</cfdi:Comprobante>");
        let c = nombre_estable("<cfdi:Comprobante>dos</cfdi:Comprobante>");
        assert_eq!(a, b, "el mismo XML dio dos nombres distintos");
        assert_ne!(a, c);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn el_nombre_del_archivo_sale_del_folio_del_comprobante() {
        let xml = crate::cfdi::xml(&{
            let mut c = crate::cfdi::Comprobante::ingreso(
                "G",
                "G20261011-01",
                "2026-10-11T23:59:00",
                crate::cfdi::Emisor {
                    rfc: "XAXX010101000".into(),
                    nombre: "Tienda".into(),
                    regimen_fiscal: "626".into(),
                    codigo_postal: "91260".into(),
                },
                crate::cfdi::Receptor::publico_general("91260"),
            );
            c.agregar(crate::cfdi::Concepto {
                clave_prod_serv: "01010101".into(),
                clave_unidad: "H87".into(),
                unidad: "Pieza".into(),
                sku: "1".into(),
                descripcion: "Venta del día".into(),
                cantidad_millis: 1000,
                valor_unitario_cents: 1000,
                descuento_cents: 0,
                tasa: crate::cfdi::TasaIva::Dieciseis,
                iva_cents: None,
            });
            c
        });
        assert_eq!(
            nombre_de(&xml),
            "G20261011-01",
            "el contador busca por folio"
        );
        // Y sin folio, cae en la huella sin tronar.
        assert_eq!(nombre_de("<cfdi:Comprobante Version=\"4.0\"/>").len(), 32);
    }

    #[test]
    fn el_pac_de_prueba_timbra_sin_internet() {
        let t = PacDePrueba
            .timbrar("<cfdi:Comprobante>x</cfdi:Comprobante>")
            .unwrap();
        // Formato de UUID: 8-4-4-4-12.
        assert_eq!(t.uuid.len(), 36);
        assert_eq!(t.uuid.matches('-').count(), 4);
        assert!(!PacDePrueba.nombre().is_empty());
    }

    #[test]
    fn una_url_http_se_reconoce_y_una_carpeta_no() {
        assert!(parece_url_http("https://api.facturapi.io/v2/invoices"));
        assert!(parece_url_http("  HTTP://pac.ejemplo.mx/timbrar  "));
        assert!(!parece_url_http("C:\\Facturas\\por-timbrar"));
        assert!(!parece_url_http("/var/facturas/salida"));
        assert!(!parece_url_http(""));
    }

    #[test]
    fn el_cliente_http_no_timbra_ni_inventa_un_uuid() {
        let pac = PacHttp::new("https://api.sw.com.mx/v4/cfdi33/issue");
        match pac.timbrar("<cfdi:Comprobante/>") {
            Err(ErrorPac::Rechazado(m)) => {
                assert!(m.contains("bloqueado"), "{m}");
                assert!(m.contains("carpetas"), "{m}");
                assert!(m.contains("https://api.sw.com.mx"), "{m}");
            }
            otro => panic!("tenía que rechazar, no hablar con nadie: {otro:?}"),
        }
        assert_eq!(pac.nombre(), "HTTP bloqueado");
    }
}
