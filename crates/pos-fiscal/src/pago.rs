//! Complemento de pagos: el CFDI que ampara el dinero que ya se pagó.
//!
//! ## Para qué existe
//!
//! Cuando le facturas a crédito a alguien (fiado, o una factura PPD), la
//! factura dice "te debo" pero no dice "ya pagué". Ese segundo aviso es otro
//! comprobante: el **complemento de pagos**, un CFDI de tipo `P`.
//!
//! Sin él, el cliente tiene una factura que no puede deducir, y se entera hasta
//! que contabilidad se la rechaza. Es el motivo número uno por el que una
//! factura a crédito "no sirve".
//!
//! ## Cómo es distinto de una factura normal
//!
//! * **No lleva importe**: `SubTotal` y `Total` son 0. El dinero no va aquí.
//! * Lleva un **concepto fijo** que dice "Pago" y nada más.
//! * Todo el contenido vive en el nodo `Complemento` → `Pagos`.
//! * Cada pago va **ligado por UUID** a las facturas que está cubriendo.
//!
//! ## Versión
//!
//! Se genera la versión **2.0** del complemento (`pago20`), que es la que el
//! SAT acepta desde 2022.

use crate::cfdi::{atributo, dinero, Receptor, TasaIva};

/// Clave de `c_ClaveProdServ` que el SAT pide para el concepto "Pago".
pub const CLAVE_PAGO: &str = "84111506";
/// Clave de `c_ClaveUnidad` para el concepto de pago: "actividad".
pub const CLAVE_UNIDAD_PAGO: &str = "ACT";

/// Una factura que este pago está cubriendo, total o en parte.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentoRelacionado {
    /// El UUID (folio fiscal) de la factura que se está pagando.
    pub uuid: String,
    /// 1 = primer pago de esta factura, 2 = el segundo, y así.
    pub num_parcialidad: i64,
    /// Cuánto se debía antes de este pago.
    pub saldo_anterior_cents: i64,
    /// Cuánto se está pagando de esta factura.
    pub pagado_cents: i64,
    /// Cuánto queda debiendo después.
    pub saldo_insoluto_cents: i64,
    /// La tasa que traía la factura original, para reportar el IVA
    /// proporcional que corresponde a este pago.
    pub tasa: TasaIva,
    /// Cuánto del total de la factura era IVA (para prorratear).
    pub iva_de_la_factura_cents: i64,
    /// Total de la factura original (para prorratear).
    pub total_de_la_factura_cents: i64,
}

/// Un pago: cuándo, cómo y cuánto.
#[derive(Clone, Debug, PartialEq)]
pub struct Pago {
    /// `AAAA-MM-DDThh:mm:ss`
    pub fecha: String,
    /// Clave de `c_FormaPago`: 01 efectivo, 03 transferencia, 04 tarjeta.
    pub forma_pago: String,
    /// Moneda en la que se pagó.
    pub moneda: String,
    /// Cuánto dinero se pagó en total (la suma de sus documentos).
    pub monto_cents: i64,
    /// Número de operación bancaria, si la hay.
    pub num_operacion: String,
    /// Las facturas que cubre este pago.
    pub documentos: Vec<DocumentoRelacionado>,
}

/// El complemento completo, listo para volverse XML.
#[derive(Clone, Debug, PartialEq)]
pub struct ComprobantePago {
    pub serie: String,
    pub folio: String,
    /// `AAAA-MM-DDThh:mm:ss`
    pub fecha: String,
    /// Código postal donde se expide.
    pub lugar_expedicion: String,
    pub emisor_rfc: String,
    pub emisor_nombre: String,
    pub emisor_regimen: String,
    pub receptor: Receptor,
    pub pagos: Vec<Pago>,
}

fn atributo_si(nombre: &str, valor: &str) -> String {
    if valor.trim().is_empty() {
        String::new()
    } else {
        atributo(nombre, valor)
    }
}

/// El XML del complemento de pagos.
pub fn xml(c: &ComprobantePago) -> String {
    let mut s = String::with_capacity(2048);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<cfdi:Comprobante");
    s.push_str(" xmlns:cfdi=\"http://www.sat.gob.mx/cfd/4\"");
    s.push_str(" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"");
    s.push_str(" xmlns:pago20=\"http://www.sat.gob.mx/Pagos20\"");
    s.push_str(
        " xsi:schemaLocation=\"http://www.sat.gob.mx/cfd/4 \
         http://www.sat.gob.mx/sitio_internet/cfd/4/cfdv40.xsd \
         http://www.sat.gob.mx/Pagos20 \
         http://www.sat.gob.mx/sitio_internet/cfd/Pagos/Pagos20.xsd\"",
    );
    s.push_str(&atributo("Version", "4.0"));
    s.push_str(&atributo_si("Serie", &c.serie));
    s.push_str(&atributo_si("Folio", &c.folio));
    s.push_str(&atributo("Fecha", &c.fecha));
    s.push_str(&atributo("Sello", ""));
    s.push_str(&atributo("NoCertificado", ""));
    s.push_str(&atributo("Certificado", ""));
    // Un comprobante de pago no lleva importe: el dinero se reporta en el
    // complemento, no aquí.
    s.push_str(&atributo("SubTotal", "0"));
    // Moneda XXX = "ninguna", porque no hay importe que convertir.
    s.push_str(&atributo("Moneda", "XXX"));
    s.push_str(&atributo("Total", "0"));
    s.push_str(&atributo("TipoDeComprobante", "P"));
    s.push_str(&atributo("Exportacion", "01"));
    s.push_str(&atributo("LugarExpedicion", &c.lugar_expedicion));
    s.push_str(">\n");

    // --- Emisor y receptor, como en cualquier CFDI ---
    s.push_str("  <cfdi:Emisor");
    s.push_str(&atributo("Rfc", &c.emisor_rfc));
    s.push_str(&atributo("Nombre", &c.emisor_nombre));
    s.push_str(&atributo("RegimenFiscal", &c.emisor_regimen));
    s.push_str("/>\n");

    s.push_str("  <cfdi:Receptor");
    s.push_str(&atributo("Rfc", &c.receptor.rfc));
    s.push_str(&atributo("Nombre", &c.receptor.nombre));
    s.push_str(&atributo(
        "DomicilioFiscalReceptor",
        &c.receptor.codigo_postal,
    ));
    s.push_str(&atributo(
        "RegimenFiscalReceptor",
        &c.receptor.regimen_fiscal,
    ));
    s.push_str(&atributo("UsoCFDI", &c.receptor.uso_cfdi));
    s.push_str("/>\n");

    // --- El concepto fijo: siempre es el mismo ---
    s.push_str("  <cfdi:Conceptos>\n");
    s.push_str("    <cfdi:Concepto");
    s.push_str(&atributo("ClaveProdServ", CLAVE_PAGO));
    s.push_str(&atributo("Cantidad", "1"));
    s.push_str(&atributo("ClaveUnidad", CLAVE_UNIDAD_PAGO));
    s.push_str(&atributo("Descripcion", "Pago"));
    s.push_str(&atributo("ValorUnitario", "0"));
    s.push_str(&atributo("Importe", "0"));
    s.push_str(&atributo("ObjetoImp", "01"));
    s.push_str("/>\n");
    s.push_str("  </cfdi:Conceptos>\n");

    // --- El complemento: aquí está todo ---
    s.push_str("  <cfdi:Complemento>\n");
    s.push_str("    <pago20:Pagos Version=\"2.0\">\n");

    let monto_total: i64 = c.pagos.iter().map(|p| p.monto_cents).sum();
    // El IVA que se reporta es el proporcional a lo que se está pagando: si
    // pagas la mitad de una factura, reportas la mitad de su IVA, no todo.
    let mut base_16: i64 = 0;
    let mut iva_16: i64 = 0;
    for pago in &c.pagos {
        for d in &pago.documentos {
            if d.tasa == TasaIva::Dieciseis && d.total_de_la_factura_cents > 0 {
                base_16 += prorratear(
                    d.pagado_cents,
                    d.total_de_la_factura_cents - d.iva_de_la_factura_cents,
                    d.total_de_la_factura_cents,
                );
                iva_16 += prorratear(
                    d.pagado_cents,
                    d.iva_de_la_factura_cents,
                    d.total_de_la_factura_cents,
                );
            }
        }
    }
    s.push_str("      <pago20:Totales");
    s.push_str(&atributo("MontoTotalPagos", &dinero(monto_total)));
    if base_16 > 0 {
        s.push_str(&atributo("TotalTrasladosBaseIVA16", &dinero(base_16)));
        s.push_str(&atributo("TotalTrasladosImpuestoIVA16", &dinero(iva_16)));
    }
    s.push_str("/>\n");

    for pago in &c.pagos {
        s.push_str("      <pago20:Pago");
        s.push_str(&atributo("FechaPago", &pago.fecha));
        s.push_str(&atributo("FormaDePagoP", &pago.forma_pago));
        s.push_str(&atributo("MonedaP", &pago.moneda));
        s.push_str(&atributo("Monto", &dinero(pago.monto_cents)));
        s.push_str(&atributo_si("NumOperacion", &pago.num_operacion));
        s.push_str(">\n");

        // El IVA de este pago, para el resumen final (ImpuestosP).
        let mut base_del_pago: i64 = 0;
        let mut iva_del_pago: i64 = 0;

        for d in &pago.documentos {
            let con_iva = d.tasa == TasaIva::Dieciseis && d.total_de_la_factura_cents > 0;
            s.push_str("        <pago20:DoctoRelacionado");
            s.push_str(&atributo("IdDocumento", &d.uuid));
            s.push_str(&atributo("MonedaDR", "MXN"));
            s.push_str(&atributo("NumParcialidad", &d.num_parcialidad.to_string()));
            s.push_str(&atributo("ImpSaldoAnt", &dinero(d.saldo_anterior_cents)));
            s.push_str(&atributo("ImpPagado", &dinero(d.pagado_cents)));
            s.push_str(&atributo(
                "ImpSaldoInsoluto",
                &dinero(d.saldo_insoluto_cents),
            ));
            // "02" = sí objeto de impuesto, y es obligatorio cuando el
            // documento trae ImpuestosDR. Si no trae, va "01" y el nodo
            // de impuestos no debe existir.
            s.push_str(&atributo("ObjetoImpDR", if con_iva { "02" } else { "01" }));
            s.push_str(">\n");
            // El IVA proporcional de este documento.
            if con_iva {
                let base = prorratear(
                    d.pagado_cents,
                    d.total_de_la_factura_cents - d.iva_de_la_factura_cents,
                    d.total_de_la_factura_cents,
                );
                let iva = prorratear(
                    d.pagado_cents,
                    d.iva_de_la_factura_cents,
                    d.total_de_la_factura_cents,
                );
                base_del_pago += base;
                iva_del_pago += iva;
                s.push_str("          <pago20:ImpuestosDR>\n");
                s.push_str("            <pago20:TrasladosDR>\n");
                s.push_str("              <pago20:TrasladoDR");
                s.push_str(&atributo("BaseDR", &dinero(base)));
                s.push_str(&atributo("ImpuestoDR", "002"));
                s.push_str(&atributo("TipoFactorDR", "Tasa"));
                s.push_str(&atributo("TasaOCuotaDR", "0.160000"));
                s.push_str(&atributo("ImporteDR", &dinero(iva)));
                s.push_str("/>\n");
                s.push_str("            </pago20:TrasladosDR>\n");
                s.push_str("          </pago20:ImpuestosDR>\n");
            }
            s.push_str("        </pago20:DoctoRelacionado>\n");
        }

        // Resumen del pago: el SAT exige que cuadre con la suma de los
        // documentos relacionados, no con el total de la factura.
        if base_del_pago > 0 {
            s.push_str("        <pago20:ImpuestosP>\n");
            s.push_str("          <pago20:TrasladosP>\n");
            s.push_str("            <pago20:TrasladoP");
            s.push_str(&atributo("BaseP", &dinero(base_del_pago)));
            s.push_str(&atributo("ImpuestoP", "002"));
            s.push_str(&atributo("TipoFactorP", "Tasa"));
            s.push_str(&atributo("TasaOCuotaP", "0.160000"));
            s.push_str(&atributo("ImporteP", &dinero(iva_del_pago)));
            s.push_str("/>\n");
            s.push_str("          </pago20:TrasladosP>\n");
            s.push_str("        </pago20:ImpuestosP>\n");
        }
        s.push_str("      </pago20:Pago>\n");
    }

    s.push_str("    </pago20:Pagos>\n");
    s.push_str("  </cfdi:Complemento>\n");
    s.push_str("</cfdi:Comprobante>\n");
    s
}

/// La parte proporcional: si de una factura de $100 con $16 de IVA pagas $50,
/// el IVA que corresponde reportar es $8, no $16.
fn prorratear(pagado: i64, parte: i64, total: i64) -> i64 {
    if total <= 0 {
        return 0;
    }
    crate::cfdi::redondear(pagado * parte, total)
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::cfdi::Receptor;

    fn receptor() -> Receptor {
        Receptor {
            rfc: "RES123456T1A".into(),
            nombre: "Restaurante El Sol".into(),
            codigo_postal: "91260".into(),
            regimen_fiscal: "601".into(),
            uso_cfdi: "G03".into(),
        }
    }

    fn comprobante(pagos: Vec<Pago>) -> ComprobantePago {
        ComprobantePago {
            serie: "CP".into(),
            folio: "1".into(),
            fecha: "2026-10-12T12:00:00".into(),
            lugar_expedicion: "91260".into(),
            emisor_rfc: "XAXX010101000".into(),
            emisor_nombre: "Abarrotes Lupita".into(),
            emisor_regimen: "626".into(),
            receptor: receptor(),
            pagos,
        }
    }

    fn documento(pagado: i64, saldo_antes: i64) -> DocumentoRelacionado {
        DocumentoRelacionado {
            uuid: "AAAABBBB-1111-2222-3333-444455556666".into(),
            num_parcialidad: 1,
            saldo_anterior_cents: saldo_antes,
            pagado_cents: pagado,
            saldo_insoluto_cents: saldo_antes - pagado,
            tasa: TasaIva::Dieciseis,
            iva_de_la_factura_cents: 89_655,
            total_de_la_factura_cents: 647_500,
        }
    }

    #[test]
    fn el_complemento_es_de_tipo_p_y_no_lleva_importe() {
        let x = xml(&comprobante(vec![Pago {
            fecha: "2026-10-12T12:00:00".into(),
            forma_pago: "01".into(),
            moneda: "MXN".into(),
            monto_cents: 500_000,
            num_operacion: String::new(),
            documentos: vec![documento(500_000, 647_500)],
        }]));
        assert!(x.contains("TipoDeComprobante=\"P\""), "{x}");
        assert!(x.contains("Total=\"0\""), "el dinero no va aquí: {x}");
        assert!(x.contains("SubTotal=\"0\""), "{x}");
        assert!(x.contains("Moneda=\"XXX\""), "{x}");
        // El concepto fijo.
        assert!(x.contains("ClaveProdServ=\"84111506\""), "{x}");
        assert!(x.contains("Descripcion=\"Pago\""), "{x}");
        // Va ligado a la factura que está pagando.
        assert!(
            x.contains("IdDocumento=\"AAAABBBB-1111-2222-3333-444455556666\""),
            "sin el UUID no se sabe qué se está pagando: {x}"
        );
        assert!(x.contains("ImpPagado=\"5000.00\""), "{x}");
    }

    #[test]
    fn el_iva_se_reporta_proporcional_no_completo() {
        // Factura de $6,475.00 con $896.55 de IVA; se pagan $5,000.00.
        let x = xml(&comprobante(vec![Pago {
            fecha: "2026-10-12T12:00:00".into(),
            forma_pago: "03".into(),
            moneda: "MXN".into(),
            monto_cents: 500_000,
            num_operacion: "1234567".into(),
            documentos: vec![documento(500_000, 647_500)],
        }]));
        // $5,000 de una factura de $6,475.00: el IVA proporcional es $692.32,
        // no los $896.55 completos de la factura.
        assert!(x.contains("ImporteDR=\"692.32\""), "{x}");
        assert!(x.contains("BaseDR=\"4307.68\""), "{x}");
        // Y la base más el impuesto dan el pago completo: ni un centavo de más.
        assert!(x.contains("Monto=\"5000.00\""), "{x}");
        assert!(x.contains("TasaOCuotaDR=\"0.160000\""), "{x}");
        assert!(x.contains("NumOperacion=\"1234567\""), "{x}");
        // Si el documento trae impuestos va "02"; con "01" el nodo de
        // impuestos no debe existir y el PAC lo rechaza.
        assert!(x.contains("ObjetoImpDR=\"02\""), "{x}");
        // Y el resumen del pago cuadra con la suma de sus documentos.
        assert!(x.contains("BaseP=\"4307.68\""), "{x}");
        assert!(x.contains("ImporteP=\"692.32\""), "{x}");
        assert!(x.contains("TasaOCuotaP=\"0.160000\""), "{x}");
    }

    #[test]
    fn un_pago_puede_cubrir_varias_facturas() {
        let mut d2 = documento(100_000, 100_000);
        d2.uuid = "OTRO-UUID".into();
        d2.num_parcialidad = 2;
        let x = xml(&comprobante(vec![Pago {
            fecha: "2026-10-12T12:00:00".into(),
            forma_pago: "01".into(),
            moneda: "MXN".into(),
            monto_cents: 600_000,
            num_operacion: String::new(),
            documentos: vec![documento(500_000, 647_500), d2],
        }]));
        // Dos documentos: se cuenta sólo la etiqueta de apertura, porque el
        // cierre trae el mismo texto.
        assert_eq!(x.matches("<pago20:DoctoRelacionado").count(), 2, "{x}");
        assert!(x.contains("OTRO-UUID"), "{x}");
        assert!(x.contains("NumParcialidad=\"2\""), "{x}");
    }

    /// Un documento exento (tasa 0%) no lleva nodo de impuestos, y su
    /// `ObjetoImpDR` tiene que decirlo: "01".
    #[test]
    fn un_documento_sin_iva_no_lleva_nodo_de_impuestos() {
        let mut d = documento(500_000, 500_000);
        d.tasa = TasaIva::Cero;
        d.iva_de_la_factura_cents = 0;
        let x = xml(&comprobante(vec![Pago {
            fecha: "2026-10-12T12:00:00".into(),
            forma_pago: "01".into(),
            moneda: "MXN".into(),
            monto_cents: 500_000,
            num_operacion: String::new(),
            documentos: vec![d],
        }]));
        assert!(x.contains("ObjetoImpDR=\"01\""), "{x}");
        assert!(!x.contains("ImpuestosDR"), "con 01 no van impuestos: {x}");
        assert!(!x.contains("ImpuestosP"), "ni resumen: {x}");
    }

    #[test]
    fn el_total_del_pago_es_la_suma_de_sus_documentos() {
        let d = documento(500_000, 647_500);
        assert_eq!(d.saldo_insoluto_cents, 147_500);
    }
}
