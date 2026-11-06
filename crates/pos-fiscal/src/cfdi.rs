//! Constructor del XML de un CFDI 4.0 (Anexo 20 del SAT).
//!
//! No timbra: **fabrica el XML**. Igual que con los tickets, construir el
//! comprobante como texto permite probarlo sin credenciales, sin internet y sin
//! pagar un timbre.
//!
//! Dos cosas que se decidieron aquí y conviene saber:
//!
//! * **Los precios del sistema traen el IVA incluido** (es como se vende en
//!   México: "$18.00" y ya). El SAT pide el importe **sin** impuesto y el IVA
//!   aparte, así que aquí se hace el desglose y se redondea a centavos.
//! * **El sello va vacío** a menos que se configure un CSD. Es lo normal cuando
//!   el PAC firma con los sellos propios del contribuyente (multi-CSD), que es
//!   como trabajan la mayoría de los PAC con clientes chicos.
//!
//! Lo que esto NO es: no valida contra el XSD del SAT ni sustituye la revisión
//! de un contador. Genera el comprobante correcto para el caso más común
//! (venta al público en general e ingreso simple).

/// Namespace del CFDI 4.0.
pub const NS_CFDI: &str = "http://www.sat.gob.mx/cfd/4";
pub const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
pub const SCHEMA: &str =
    "http://www.sat.gob.mx/cfd/4 http://www.sat.gob.mx/sitio_internet/cfd/4/cfdv40.xsd";

/// RFC genérico para "público en general" (Regla 2.7.1.24).
pub const RFC_PUBLICO: &str = "XAXX010101000";
/// Nombre que exige el SAT para el público en general.
pub const NOMBRE_PUBLICO: &str = "PUBLICO EN GENERAL";

/// Tasa de IVA. El SAT la pide con seis decimales ("0.160000").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TasaIva {
    Dieciseis,
    Ocho,
    Cero,
    Exento,
}

impl TasaIva {
    /// Texto del atributo `TasaOCuota`. `None` cuando es exento (allí el
    /// `TipoFactor` es "Exento" y no se manda tasa).
    pub fn tasa(self) -> Option<&'static str> {
        match self {
            TasaIva::Dieciseis => Some("0.160000"),
            TasaIva::Ocho => Some("0.080000"),
            TasaIva::Cero => Some("0.000000"),
            TasaIva::Exento => None,
        }
    }

    pub fn tipo_factor(self) -> &'static str {
        if self == TasaIva::Exento {
            "Exento"
        } else {
            "Tasa"
        }
    }

    /// Objeto del impuesto: "02" = sí es objeto, "01" = no lo es.
    pub fn objeto(self) -> &'static str {
        if self == TasaIva::Exento {
            "01"
        } else {
            "02"
        }
    }

    /// IVA en centavos sobre una base. Exento no cobra nada.
    pub fn importe(self, base_cents: i64) -> i64 {
        match self {
            TasaIva::Dieciseis => redondear(base_cents * 16, 100),
            TasaIva::Ocho => redondear(base_cents * 8, 100),
            TasaIva::Cero | TasaIva::Exento => 0,
        }
    }
}

/// Redondeo "half up": el que usa el SAT y el que evita que un centavo se
/// pierda por culpa del punto flotante. Aquí no hay `f64` en ninguna parte.
pub fn redondear(numerador: i64, denominador: i64) -> i64 {
    if denominador == 0 {
        return 0;
    }
    let negativo = (numerador < 0) ^ (denominador < 0);
    let n = numerador.abs();
    let d = denominador.abs();
    let cociente = n / d;
    let residuo = n % d;
    let sube = residuo * 2 >= d;
    let r = if sube { cociente + 1 } else { cociente };
    if negativo {
        -r
    } else {
        r
    }
}

/// De qué trata el comprobante.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tipo {
    /// Venta: entra dinero.
    #[default]
    Ingreso,
    /// Nota de crédito: sale dinero (una devolución, un descuento posterior).
    Egreso,
}

impl Tipo {
    /// La clave del catálogo `c_TipoDeComprobante`.
    pub fn clave(self) -> &'static str {
        match self {
            Tipo::Ingreso => "I",
            Tipo::Egreso => "E",
        }
    }
}

/// Los comprobantes a los que se liga éste (catálogo `c_TipoRelacion`).
///
/// `01` = nota de crédito de los documentos relacionados.
/// `03` = devolución de mercancía sobre facturas previas.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relacion {
    pub tipo_relacion: String,
    /// Los UUID de las facturas que se están corrigiendo.
    pub uuids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emisor {
    pub rfc: String,
    pub nombre: String,
    /// Clave del catálogo `c_RegimenFiscal` (601, 612, 626...).
    pub regimen_fiscal: String,
    /// Código postal del domicilio fiscal: va en `LugarExpedicion`.
    pub codigo_postal: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receptor {
    pub rfc: String,
    pub nombre: String,
    /// En 4.0 es obligatorio: el código postal del receptor.
    pub codigo_postal: String,
    /// Clave de `c_RegimenFiscal` del receptor (616 para público en general).
    pub regimen_fiscal: String,
    /// Clave de `c_UsoCFDI` (S01 para público en general, G03 gastos...).
    pub uso_cfdi: String,
}

impl Receptor {
    /// El receptor de una factura global: público en general.
    pub fn publico_general(codigo_postal_emisor: &str) -> Receptor {
        Receptor {
            rfc: RFC_PUBLICO.to_string(),
            nombre: NOMBRE_PUBLICO.to_string(),
            codigo_postal: codigo_postal_emisor.to_string(),
            regimen_fiscal: "616".to_string(),
            uso_cfdi: "S01".to_string(),
        }
    }
}

/// Un renglón del comprobante.
#[derive(Clone, Debug, PartialEq)]
pub struct Concepto {
    /// Clave del catálogo `c_ClaveProdServ`.
    pub clave_prod_serv: String,
    /// Clave de `c_ClaveUnidad` (H87 = pieza).
    pub clave_unidad: String,
    pub unidad: String,
    pub sku: String,
    pub descripcion: String,
    /// Cantidad en milésimos (2.500 kg = 2500).
    pub cantidad_millis: i64,
    /// Precio unitario **sin** impuesto, en centavos.
    pub valor_unitario_cents: i64,
    /// Descuento de la línea, en centavos.
    pub descuento_cents: i64,
    pub tasa: TasaIva,
    /// El IVA exacto de la línea, cuando quien arma el comprobante ya lo sabe.
    ///
    /// Importa por esto: un precio con IVA incluido ($36.00) **no** se puede
    /// partir en neto + IVA sin que sobre o falte un centavo ($31.03 + $4.96
    /// = $35.99 y $31.04 + $4.97 = $36.01). Si aquí se deja en `None`, el IVA
    /// se recalcula como `base × tasa` y el total de la factura deja de ser lo
    /// que el cliente pagó.
    pub iva_cents: Option<i64>,
}

impl Concepto {
    /// Importe de la línea: cantidad × valor unitario, sin descuento ni IVA.
    pub fn importe_cents(&self) -> i64 {
        redondear(self.valor_unitario_cents * self.cantidad_millis, 1000)
    }

    /// Base del IVA: el importe ya con el descuento aplicado.
    pub fn base_cents(&self) -> i64 {
        (self.importe_cents() - self.descuento_cents).max(0)
    }

    pub fn iva_cents(&self) -> i64 {
        match self.iva_cents {
            Some(v) => v,
            None => self.tasa.importe(self.base_cents()),
        }
    }
}

/// El comprobante completo.
#[derive(Clone, Debug, PartialEq)]
pub struct Comprobante {
    pub serie: String,
    pub folio: String,
    /// Fecha y hora local de expedición: `AAAA-MM-DDThh:mm:ss`.
    pub fecha: String,
    /// Clave de `c_FormaPago` (01 efectivo, 04 tarjeta de crédito...).
    pub forma_pago: String,
    /// "PUE" (una exhibición) o "PPD" (parcialidades).
    pub metodo_pago: String,
    /// Ingreso (venta) o egreso (nota de crédito).
    pub tipo: Tipo,
    /// A qué facturas corrige ésta. Sólo aplica en notas de crédito.
    pub relacion: Option<Relacion>,
    pub emisor: Emisor,
    pub receptor: Receptor,
    pub conceptos: Vec<Concepto>,
    // --- sólo para factura global ---
    pub periodicidad: Option<String>,
    pub meses: Option<String>,
    pub anio: Option<String>,
    // --- sello, si se firma con CSD propio ---
    pub sello: String,
    pub no_certificado: String,
    pub certificado: String,
}

/// Cuánto IVA se trasladó de una sola tasa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrupoIva {
    pub tasa: TasaIva,
    /// La suma de las bases de los conceptos con esa tasa.
    pub base_cents: i64,
    pub importe_cents: i64,
}

impl Comprobante {
    /// Ingreso simple: venta de mercancías.
    pub fn ingreso(
        serie: &str,
        folio: &str,
        fecha: &str,
        emisor: Emisor,
        receptor: Receptor,
    ) -> Self {
        Comprobante {
            serie: serie.to_string(),
            folio: folio.to_string(),
            fecha: fecha.to_string(),
            forma_pago: "01".to_string(),
            metodo_pago: "PUE".to_string(),
            tipo: Tipo::Ingreso,
            relacion: None,
            emisor,
            receptor,
            conceptos: Vec::new(),
            periodicidad: None,
            meses: None,
            anio: None,
            sello: String::new(),
            no_certificado: String::new(),
            certificado: String::new(),
        }
    }

    /// Nota de crédito: devuelve dinero de una factura ya timbrada.
    ///
    /// Va ligada a la factura original por su UUID, porque así es como el SAT
    /// sabe qué se está restando de qué. Sin ese vínculo, la nota no sirve.
    pub fn nota_credito(
        serie: &str,
        folio: &str,
        fecha: &str,
        emisor: Emisor,
        receptor: Receptor,
        relacion: Relacion,
    ) -> Self {
        let mut c = Comprobante::ingreso(serie, folio, fecha, emisor, receptor);
        c.tipo = Tipo::Egreso;
        // En un egreso no hay forma de pago ni método: no entra dinero.
        c.forma_pago.clear();
        c.metodo_pago.clear();
        c.relacion = Some(relacion);
        c
    }

    /// Lo marca como factura global. `Periodicidad` viene del catálogo
    /// c_Periodicidad: 01 diario, 02 semanal, 04 mensual.
    pub fn como_global(&mut self, periodicidad: &str, meses: &str, anio: &str) -> &mut Self {
        self.periodicidad = Some(periodicidad.to_string());
        self.meses = Some(meses.to_string());
        self.anio = Some(anio.to_string());
        self
    }

    pub fn agregar(&mut self, c: Concepto) -> &mut Self {
        self.conceptos.push(c);
        self
    }

    /// Suma de los importes de los conceptos, antes de descuentos.
    pub fn subtotal_cents(&self) -> i64 {
        self.conceptos.iter().map(|c| c.importe_cents()).sum()
    }

    pub fn descuento_cents(&self) -> i64 {
        self.conceptos.iter().map(|c| c.descuento_cents).sum()
    }

    pub fn iva_cents(&self) -> i64 {
        self.conceptos.iter().map(|c| c.iva_cents()).sum()
    }

    /// Total = subtotal − descuento + impuestos trasladados.
    pub fn total_cents(&self) -> i64 {
        self.subtotal_cents() - self.descuento_cents() + self.iva_cents()
    }

    /// Junta los conceptos por tasa, para el nodo `Impuestos` del comprobante.
    ///
    /// Los exentos no se totalizan aquí: van renglón por renglón, como pide el
    /// SAT, y no suman al total de traslados.
    pub fn grupos_de_iva(&self) -> Vec<GrupoIva> {
        let mut grupos: Vec<GrupoIva> = Vec::new();
        for con in &self.conceptos {
            if con.tasa == TasaIva::Exento {
                continue;
            }
            match grupos.iter_mut().find(|g| g.tasa == con.tasa) {
                Some(g) => {
                    g.base_cents += con.base_cents();
                    g.importe_cents += con.iva_cents();
                }
                None => grupos.push(GrupoIva {
                    tasa: con.tasa,
                    base_cents: con.base_cents(),
                    importe_cents: con.iva_cents(),
                }),
            }
        }
        grupos
    }
}

/// Centavos a texto con dos decimales, como lo pide el SAT.
pub fn dinero(cents: i64) -> String {
    let negativo = cents < 0;
    let c = cents.abs();
    format!(
        "{}{}.{:02}",
        if negativo { "-" } else { "" },
        c / 100,
        c % 100
    )
}

/// Milésimos a texto: 2500 → "2.500", 1000 → "1".
pub fn cantidad(millis: i64) -> String {
    let entero = millis / 1000;
    let fraccion = millis.abs() % 1000;
    if fraccion == 0 {
        format!("{entero}")
    } else {
        format!("{entero}.{fraccion:03}")
    }
}

/// Escapa el texto para que no rompa el XML.
///
/// Importa más de lo que parece: un producto que se llame "Pan & Dulce" o
/// "Café <1 kg>" produce un XML inválido si no se escapa, y el PAC lo rechaza
/// con un error que nadie entiende.
pub fn escapar(texto: &str) -> String {
    let mut s = String::with_capacity(texto.len());
    for c in texto.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&apos;"),
            // El SAT no acepta caracteres de control en el comprobante.
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => s.push(c),
        }
    }
    s
}

pub(crate) fn atributo(nombre: &str, valor: &str) -> String {
    format!(" {nombre}=\"{}\"", escapar(valor))
}

fn atributo_si(nombre: &str, valor: &str) -> String {
    if valor.trim().is_empty() {
        String::new()
    } else {
        atributo(nombre, valor)
    }
}

/// Construye el XML del comprobante.
pub fn xml(c: &Comprobante) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

    s.push_str(&format!("<cfdi:Comprobante xmlns:cfdi=\"{NS_CFDI}\""));
    s.push_str(&format!(" xmlns:xsi=\"{NS_XSI}\""));
    s.push_str(&format!(" xsi:schemaLocation=\"{SCHEMA}\""));
    s.push_str(&atributo("Version", "4.0"));
    s.push_str(&atributo_si("Serie", &c.serie));
    s.push_str(&atributo_si("Folio", &c.folio));
    s.push_str(&atributo("Fecha", &c.fecha));
    s.push_str(&atributo_si("Sello", &c.sello));
    if c.tipo == Tipo::Ingreso {
        s.push_str(&atributo_si("FormaPago", &c.forma_pago));
    }
    s.push_str(&atributo_si("NoCertificado", &c.no_certificado));
    s.push_str(&atributo_si("Certificado", &c.certificado));
    // En moneda nacional no se manda TipoCambio: el SAT lo rechaza.
    s.push_str(&atributo("Moneda", "MXN"));
    if !c.descuento_cents().to_string().is_empty() && c.descuento_cents() > 0 {
        s.push_str(&atributo("Descuento", &dinero(c.descuento_cents())));
    }
    s.push_str(&atributo("SubTotal", &dinero(c.subtotal_cents())));
    s.push_str(&atributo("Total", &dinero(c.total_cents())));
    s.push_str(&atributo("TipoDeComprobante", c.tipo.clave()));
    s.push_str(&atributo("Exportacion", "01"));
    // En una nota de crédito no se manda ni forma ni método de pago: el SAT
    // los rechaza en un egreso.
    if c.tipo == Tipo::Ingreso {
        s.push_str(&atributo("MetodoPago", &c.metodo_pago));
    }
    s.push_str(&atributo("LugarExpedicion", &c.emisor.codigo_postal));
    // La factura global lleva estos tres (Anexo 20, versión 4.0).
    if let Some(p) = &c.periodicidad {
        s.push_str(&atributo("Periodicidad", p));
    }
    if let Some(m) = &c.meses {
        s.push_str(&atributo("Meses", m));
    }
    if let Some(a) = &c.anio {
        s.push_str(&atributo("Año", a));
    }
    s.push_str(">\n");

    // --- A qué factura corrige ésta (sólo notas de crédito) ---
    if let Some(rel) = &c.relacion {
        if !rel.uuids.is_empty() {
            s.push_str("  <cfdi:CfdiRelacionados");
            s.push_str(&atributo("TipoRelacion", &rel.tipo_relacion));
            s.push_str(">\n");
            for uuid in &rel.uuids {
                s.push_str("    <cfdi:CfdiRelacionado");
                s.push_str(&atributo("UUID", uuid));
                s.push_str("/>\n");
            }
            s.push_str("  </cfdi:CfdiRelacionados>\n");
        }
    }

    // --- Emisor ---
    s.push_str("  <cfdi:Emisor");
    s.push_str(&atributo("Rfc", &c.emisor.rfc));
    s.push_str(&atributo("Nombre", &c.emisor.nombre));
    s.push_str(&atributo("RegimenFiscal", &c.emisor.regimen_fiscal));
    s.push_str("/>\n");

    // --- Receptor ---
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

    // --- Conceptos ---
    s.push_str("  <cfdi:Conceptos>\n");
    for con in &c.conceptos {
        s.push_str("    <cfdi:Concepto");
        s.push_str(&atributo("ClaveProdServ", &con.clave_prod_serv));
        s.push_str(&atributo_si("NoIdentificacion", &con.sku));
        s.push_str(&atributo("Cantidad", &cantidad(con.cantidad_millis)));
        s.push_str(&atributo("ClaveUnidad", &con.clave_unidad));
        s.push_str(&atributo_si("Unidad", &con.unidad));
        s.push_str(&atributo("Descripcion", &con.descripcion));
        s.push_str(&atributo(
            "ValorUnitario",
            &dinero(con.valor_unitario_cents),
        ));
        s.push_str(&atributo("Importe", &dinero(con.importe_cents())));
        if con.descuento_cents > 0 {
            s.push_str(&atributo("Descuento", &dinero(con.descuento_cents)));
        }
        s.push_str(&atributo("ObjetoImp", con.tasa.objeto()));
        s.push_str(">\n");

        if con.tasa != TasaIva::Cero || con.tasa == TasaIva::Cero {
            s.push_str(
                "      <cfdi:Impuestos>\n        <cfdi:Traslados>\n          <cfdi:Traslado",
            );
            s.push_str(&atributo("Base", &dinero(con.base_cents())));
            s.push_str(&atributo("Impuesto", "002"));
            s.push_str(&atributo("TipoFactor", con.tasa.tipo_factor()));
            if let Some(t) = con.tasa.tasa() {
                s.push_str(&atributo("TasaOCuota", t));
            }
            s.push_str(&atributo("Importe", &dinero(con.iva_cents())));
            s.push_str("/>\n        </cfdi:Traslados>\n      </cfdi:Impuestos>\n");
        }
        s.push_str("    </cfdi:Concepto>\n");
    }
    s.push_str("  </cfdi:Conceptos>\n");

    // --- Impuestos del comprobante ---
    //
    // Un renglón por cada tasa distinta: una factura con cosas al 16% y cosas
    // al 0% lleva dos `Traslado`, no uno solo que las mezcle. (Antes se ponía
    // un único traslado al 16% y las facturas con mezcla de tasas mentían.)
    let grupos = c.grupos_de_iva();
    let iva = grupos.iter().map(|g| g.importe_cents).sum::<i64>();
    s.push_str("  <cfdi:Impuestos");
    if iva > 0 {
        s.push_str(&atributo("TotalImpuestosTrasladados", &dinero(iva)));
    }
    s.push_str(">\n");
    if !grupos.is_empty() {
        s.push_str("    <cfdi:Traslados>\n");
        for g in &grupos {
            s.push_str("      <cfdi:Traslado");
            s.push_str(&atributo("Base", &dinero(g.base_cents)));
            s.push_str(&atributo("Impuesto", "002"));
            s.push_str(&atributo("TipoFactor", g.tasa.tipo_factor()));
            if let Some(t) = g.tasa.tasa() {
                s.push_str(&atributo("TasaOCuota", t));
            }
            s.push_str(&atributo("Importe", &dinero(g.importe_cents)));
            s.push_str("/>\n");
        }
        s.push_str("    </cfdi:Traslados>\n");
    }
    s.push_str("  </cfdi:Impuestos>\n");

    s.push_str("</cfdi:Comprobante>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emisor() -> Emisor {
        Emisor {
            rfc: "XAXX010101000".into(),
            nombre: "Abarrotes Lupita".into(),
            regimen_fiscal: "626".into(),
            codigo_postal: "91260".into(),
        }
    }

    fn receptor() -> Receptor {
        Receptor::publico_general("91260")
    }

    fn comprobante() -> Comprobante {
        Comprobante::ingreso("A", "123", "2026-10-11T18:42:00", emisor(), receptor())
    }

    #[test]
    fn el_xml_abre_y_cierra_bien() {
        let mut c = comprobante();
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "7501".into(),
            descripcion: "Coca-Cola 600 ml".into(),
            cantidad_millis: 2000,
            valor_unitario_cents: 1552, // $18.00 con IVA → $15.52 sin IVA
            descuento_cents: 0,
            tasa: TasaIva::Dieciseis,
            iva_cents: None,
        });
        let x = xml(&c);
        assert_eq!(esta_bien_formado(&x), Ok(()), "el XML está chueco:\n{x}");
        assert!(x.starts_with("<?xml"));
        assert!(x.trim_end().ends_with("</cfdi:Comprobante>"));
    }

    /// Revisa que las etiquetas abran y cierren en el orden correcto.
    ///
    /// No es un parser XML completo: es lo justo para cazar el error que de
    /// verdad se comete al armar el comprobante a mano, que es dejar una
    /// etiqueta abierta o cerrar la que no era.
    fn esta_bien_formado(xml: &str) -> Result<(), String> {
        let mut pila: Vec<&str> = Vec::new();
        let mut resto = xml;
        while let Some(inicio) = resto.find('<') {
            let fin = match resto[inicio..].find('>') {
                Some(f) => inicio + f,
                None => return Err("hay un < sin su >".to_string()),
            };
            let etiqueta = &resto[inicio + 1..fin];
            resto = &resto[fin + 1..];
            if etiqueta.starts_with("?xml") || etiqueta.starts_with('!') {
                continue;
            }
            if etiqueta.ends_with('/') {
                continue; // se abre y se cierra: no toca la pila
            }
            if let Some(nombre) = etiqueta.strip_prefix('/') {
                match pila.pop() {
                    Some(abierta) if abierta == nombre => {}
                    otra => return Err(format!("cerró <{nombre}> pero estaba abierta {otra:?}")),
                }
            } else {
                pila.push(etiqueta.split_whitespace().next().unwrap_or(""));
            }
        }
        if pila.is_empty() {
            Ok(())
        } else {
            Err(format!("quedaron etiquetas abiertas: {pila:?}"))
        }
    }

    #[test]
    fn el_total_cuadra_subtotal_menos_descuento_mas_iva() {
        let mut c = comprobante();
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "1".into(),
            descripcion: "Pan".into(),
            cantidad_millis: 3000,
            valor_unitario_cents: 1000, // $10.00
            descuento_cents: 500,       // -$5.00
            tasa: TasaIva::Dieciseis,
            iva_cents: None,
        });
        // 3 × 10.00 = 30.00 − 5.00 = 25.00 + IVA 4.00 = 29.00
        assert_eq!(c.subtotal_cents(), 3000);
        assert_eq!(c.descuento_cents(), 500);
        assert_eq!(c.iva_cents(), 400);
        assert_eq!(c.total_cents(), 2900);
        let x = xml(&c);
        assert!(x.contains("SubTotal=\"30.00\""), "{}", x);
        assert!(x.contains("Descuento=\"5.00\""));
        assert!(x.contains("Total=\"29.00\""));
        assert!(x.contains("TotalImpuestosTrasladados=\"4.00\""));
    }

    #[test]
    fn el_iva_se_desglosa_de_un_precio_que_ya_lo_incluia() {
        // $18.00 con IVA = $15.5172 sin IVA; en centavos, 1552.
        let neto = redondear(1800 * 100, 116);
        assert_eq!(neto, 1552);
        assert_eq!(TasaIva::Dieciseis.importe(neto), 248);
        // 15.52 + 2.48 = 18.00: ni un centavo de más ni de menos.
        assert_eq!(neto + TasaIva::Dieciseis.importe(neto), 1800);
    }

    #[test]
    fn los_caracteres_especiales_no_rompen_el_xml() {
        let mut c = comprobante();
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "X&1".into(),
            descripcion: "Pan & Dulce <1 kg> \"especial\"".into(),
            cantidad_millis: 1000,
            valor_unitario_cents: 100,
            descuento_cents: 0,
            tasa: TasaIva::Dieciseis,
            iva_cents: None,
        });
        let x = xml(&c);
        assert!(
            x.contains("Pan &amp; Dulce &lt;1 kg&gt; &quot;especial&quot;"),
            "{}",
            x
        );
        // Ni un & suelto que no sea una entidad.
        let cuerpo = x.split_once("<cfdi:Comprobante").unwrap().1;
        for parte in cuerpo.split('&').skip(1) {
            assert!(
                parte.starts_with("amp;")
                    || parte.starts_with("lt;")
                    || parte.starts_with("gt;")
                    || parte.starts_with("quot;")
                    || parte.starts_with("apos;"),
                "hay un & mal escapado: {x}"
            );
        }
    }

    #[test]
    fn los_acentos_van_tal_cual_porque_el_xml_es_utf8() {
        let mut c = comprobante();
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "1".into(),
            descripcion: "Jamón serrano".into(),
            cantidad_millis: 1000,
            valor_unitario_cents: 100,
            descuento_cents: 0,
            tasa: TasaIva::Dieciseis,
            iva_cents: None,
        });
        let x = xml(&c);
        assert!(x.contains("Jamón serrano"), "se perdió el acento");
        assert!(x.contains("encoding=\"UTF-8\""));
    }

    #[test]
    fn en_moneda_nacional_no_se_manda_tipo_de_cambio() {
        let mut c = comprobante();
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "1".into(),
            descripcion: "Algo".into(),
            cantidad_millis: 1000,
            valor_unitario_cents: 100,
            descuento_cents: 0,
            tasa: TasaIva::Dieciseis,
            iva_cents: None,
        });
        let x = xml(&c);
        assert!(!x.contains("TipoCambio"), "el SAT lo rechaza en MXN");
        assert!(x.contains("Moneda=\"MXN\""));
    }

    #[test]
    fn la_factura_global_lleva_periodicidad_meses_y_anio() {
        let mut c = comprobante();
        c.como_global("01", "10", "2026");
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "1".into(),
            descripcion: "Ventas del día".into(),
            cantidad_millis: 1000,
            valor_unitario_cents: 100,
            descuento_cents: 0,
            tasa: TasaIva::Dieciseis,
            iva_cents: None,
        });
        let x = xml(&c);
        assert!(x.contains("Periodicidad=\"01\""), "{}", x);
        assert!(x.contains("Meses=\"10\""));
        assert!(x.contains("Año=\"2026\""));
        // Y va al público en general.
        assert!(x.contains("Rfc=\"XAXX010101000\""));
        assert!(x.contains("UsoCFDI=\"S01\""));
        assert!(x.contains("RegimenFiscalReceptor=\"616\""));
    }

    #[test]
    fn un_comprobante_normal_no_lleva_datos_de_global() {
        let mut c = comprobante();
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "1".into(),
            descripcion: "Algo".into(),
            cantidad_millis: 1000,
            valor_unitario_cents: 100,
            descuento_cents: 0,
            tasa: TasaIva::Dieciseis,
            iva_cents: None,
        });
        let x = xml(&c);
        assert!(!x.contains("Periodicidad"));
        assert!(!x.contains("Año="));
    }

    #[test]
    fn la_tasa_cero_y_el_exento_salen_como_pide_el_sat() {
        let mut c = comprobante();
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "1".into(),
            descripcion: "Libro (0%)".into(),
            cantidad_millis: 1000,
            valor_unitario_cents: 100,
            descuento_cents: 0,
            tasa: TasaIva::Cero,
            iva_cents: None,
        });
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "2".into(),
            descripcion: "Medicamento (exento)".into(),
            cantidad_millis: 1000,
            valor_unitario_cents: 100,
            descuento_cents: 0,
            tasa: TasaIva::Exento,
            iva_cents: None,
        });
        let x = xml(&c);
        assert!(x.contains("TasaOCuota=\"0.000000\""), "{}", x);
        assert!(x.contains("TipoFactor=\"Exento\""));
        assert!(x.contains("ObjetoImp=\"02\""));
        // Ningún total de traslados: nadie pagó IVA.
        assert!(!x.contains("TotalImpuestosTrasladados"));
        assert_eq!(c.total_cents(), 200);
    }

    #[test]
    fn las_cantidades_con_decimales_salen_bien() {
        assert_eq!(cantidad(1000), "1");
        assert_eq!(cantidad(2500), "2.500");
        assert_eq!(cantidad(100), "0.100");
        assert_eq!(dinero(1800), "18.00");
        assert_eq!(dinero(5), "0.05");
        assert_eq!(dinero(-300), "-3.00");
    }

    #[test]
    fn el_redondeo_es_half_up_y_nunca_pierde_centavos() {
        assert_eq!(redondear(5, 2), 3); // 2.5 → 3
        assert_eq!(redondear(4, 2), 2); // 2.0 → 2
        assert_eq!(redondear(-5, 2), -3);
        assert_eq!(redondear(7, 0), 0); // división entre cero no truena
                                        // El caso que duele: 3 × $15.52 sin redondear pierde un centavo.
        let tres = 1552 * 3;
        assert_eq!(redondear(tres, 1), 4656);
    }

    #[test]
    fn el_xml_lleva_los_namespaces_y_el_schema() {
        let mut c = comprobante();
        c.agregar(Concepto {
            clave_prod_serv: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad: "Pieza".into(),
            sku: "1".into(),
            descripcion: "Algo".into(),
            cantidad_millis: 1000,
            valor_unitario_cents: 100,
            descuento_cents: 0,
            tasa: TasaIva::Dieciseis,
            iva_cents: None,
        });
        let x = xml(&c);
        assert!(x.contains(&format!("xmlns:cfdi=\"{NS_CFDI}\"")));
        assert!(x.contains(&format!("xmlns:xsi=\"{NS_XSI}\"")));
        assert!(x.contains("xsi:schemaLocation"));
        assert!(x.contains("Version=\"4.0\""));
        assert!(x.contains("Exportacion=\"01\""));
        assert!(x.contains("MetodoPago=\"PUE\""));
        assert!(x.contains("LugarExpedicion=\"91260\""));
    }
}
