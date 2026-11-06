//! Factura global: un comprobante por las ventas del periodo al público en
//! general (Regla 2.7.1.24 de la RMF).
//!
//! Es el caso de uso número uno de una tienda: la gente paga y se va, nadie
//! pide factura, y al final del día hay que emitir una sola factura por todo
//! eso, a nombre del público en general.
//!
//! Todo aquí trabaja con centavos y nunca con `f64`, por una razón concreta:
//! **los centavos tienen que cuadrar**. Si la factura global dice $4,318.27 y
//! la suma de los tickets del día dice $4,318.28, el contador pierde la tarde.
//! Por eso el IVA se calcula por complemento: `neto + iva == bruto` siempre,
//! sin excepción, aunque el IVA quede un centavo arriba o abajo del 16%
//! exacto (el SAT lo tolera; no cuadrar el total, no).

use crate::cfdi::{redondear, Comprobante, Concepto, Emisor, Receptor, Relacion, TasaIva};

/// Un renglón de un ticket, tal como lo guarda el punto de venta: con el IVA
/// incluido en el precio, que es como se vende en México.
#[derive(Clone, Debug, PartialEq)]
pub struct LineaTicket {
    pub sku: String,
    pub descripcion: String,
    pub cantidad_millis: i64,
    /// Precio unitario **con** impuesto, en centavos.
    pub precio_bruto_cents: i64,
    /// Descuento **con** impuesto, en centavos.
    pub descuento_cents: i64,
    pub tasa: TasaIva,
    /// Clave del catálogo `c_ClaveProdServ` de este producto.
    pub clave_sat: String,
    /// Clave de `c_ClaveUnidad` (H87 = pieza).
    pub clave_unidad: String,
    /// Cómo se nombra la unidad: "Pieza", "Kilo", "Litro".
    pub unidad: String,
}

impl LineaTicket {
    /// Importe de la línea con IVA incluido y con el descuento ya aplicado.
    pub fn bruto_cents(&self) -> i64 {
        let bruto = redondear(self.precio_bruto_cents * self.cantidad_millis, 1000);
        (bruto - self.descuento_cents).max(0)
    }

    /// Importe sin IVA. Para 16% es redondear a 100/116 del bruto.
    pub fn neto_cents(&self) -> i64 {
        match self.tasa {
            TasaIva::Dieciseis => redondear(self.bruto_cents() * 100, 116),
            TasaIva::Ocho => redondear(self.bruto_cents() * 100, 108),
            // Tasa cero y exento: el neto es todo el importe.
            TasaIva::Cero | TasaIva::Exento => self.bruto_cents(),
        }
    }

    /// El IVA por complemento: así `neto + iva` da el importe exacto del
    /// ticket, ni un centavo de más ni de menos.
    pub fn iva_cents(&self) -> i64 {
        self.bruto_cents() - self.neto_cents()
    }

    /// Una parte de esta línea: la misma cosa, pero devuelta a medias.
    ///
    /// El descuento se reparte proporcional, porque si a cinco cosas les
    /// quitaste $2.00, a dos se les quitan 80 centavos, no dos pesos.
    pub fn parcial(&self, cantidad_millis: i64) -> LineaTicket {
        let cantidad = cantidad_millis.clamp(0, self.cantidad_millis);
        let descuento = if self.cantidad_millis <= 0 {
            0
        } else {
            // Redondeo al centavo más cercano, no hacia abajo: repartir
            // siempre hacia abajo le regala centavos al cliente en cada
            // devolución parcial.
            redondear(self.descuento_cents * cantidad, self.cantidad_millis)
        };
        LineaTicket {
            cantidad_millis: cantidad,
            descuento_cents: descuento,
            ..self.clone()
        }
    }

    /// Total de la línea, con IVA. Es lo que pagó el cliente.
    pub fn total_cents(&self) -> i64 {
        self.bruto_cents()
    }
}

/// Clave genérica de `c_ClaveProdServ`: "no existe en el catálogo".
///
/// Es la que se usa cuando el producto no tiene la suya, y es válida: el SAT
/// la acepta mientras el importe y los impuestos estén bien.
pub const CLAVE_SAT_GENERICA: &str = "01010101";
/// Clave de `c_ClaveUnidad` para pieza.
pub const CLAVE_UNIDAD_PIEZA: &str = "H87";

/// Un ticket completo: las líneas que compró un cliente.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ticket {
    pub folio: String,
    pub lineas: Vec<LineaTicket>,
}

impl Ticket {
    /// Lo que pagó el cliente por todo el ticket.
    pub fn total_cents(&self) -> i64 {
        self.lineas.iter().map(|l| l.total_cents()).sum()
    }
}

/// Arma la nota de crédito de una devolución.
///
/// La devolución se describe igual que una venta (con precios que ya traen el
/// IVA dentro) y el importe se desglosa con la misma regla, así que el total de
/// la nota es exactamente lo que se devolvió.
///
/// `relacion` trae el UUID de la factura que se está corrigiendo: sin eso el
/// SAT no sabe de qué se está restando y la nota no vale.
pub fn nota_credito(
    emisor: &Emisor,
    receptor: &Receptor,
    ticket: &Ticket,
    fecha: &str,
    relacion: Relacion,
    folio: &str,
) -> Comprobante {
    let mut c = Comprobante::nota_credito(
        "NC",
        folio,
        fecha,
        emisor.clone(),
        receptor.clone(),
        relacion,
    );
    for concepto in conceptos_por_linea(ticket) {
        c.agregar(concepto);
    }
    c
}

/// Un renglón por cada cosa devuelta, en vez de juntar por tasa como hace la
/// factura global: en una nota de crédito importa ver **qué** se está
/// devolviendo.
///
/// La cantidad del renglón siempre es 1 y el precio unitario es el importe neto
/// de esa línea, con la cantidad dicha en la descripción. Es deliberado: un
/// precio con IVA incluido no reparte exacto entre piezas ($15.52 × 2 =
/// $31.04, no $31.03), y poner la cantidad como viene haría que la nota dijera
/// un importe distinto al dinero que de veras se devolvió.
pub fn conceptos_por_linea(ticket: &Ticket) -> Vec<Concepto> {
    ticket
        .lineas
        .iter()
        .filter(|l| l.cantidad_millis > 0)
        .map(|l| Concepto {
            clave_prod_serv: if l.clave_sat.trim().is_empty() {
                CLAVE_SAT_GENERICA.to_string()
            } else {
                l.clave_sat.trim().to_string()
            },
            clave_unidad: if l.clave_unidad.trim().is_empty() {
                CLAVE_UNIDAD_PIEZA.to_string()
            } else {
                l.clave_unidad.trim().to_string()
            },
            unidad: if l.unidad.trim().is_empty() {
                "Pieza".to_string()
            } else {
                l.unidad.trim().to_string()
            },
            sku: l.sku.clone(),
            descripcion: format!("{} x {}", cantidad_txt(l.cantidad_millis), l.descripcion),
            cantidad_millis: 1000,
            valor_unitario_cents: l.neto_cents(),
            descuento_cents: 0,
            tasa: l.tasa,
            // El IVA dicho con número, por la misma razón de siempre.
            iva_cents: Some(l.iva_cents()),
        })
        .collect()
}

/// 2000 -> "2", 2500 -> "2.5". Sin ceros de más.
fn cantidad_txt(millis: i64) -> String {
    if millis % 1000 == 0 {
        format!("{}", millis / 1000)
    } else {
        let t = format!("{:.3}", millis as f64 / 1000.0);
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Junta las líneas de un ticket por tasa: un ticket con cosas al 16% y cosas
/// exentas produce dos renglones en el comprobante, no uno.
pub fn conceptos_de(ticket: &Ticket) -> Vec<Concepto> {
    // Se guarda el neto de la tasa en centavos, no el importe completo.
    let mut por_tasa: Vec<(TasaIva, i64, i64)> = Vec::new(); // tasa, neto, iva
    for linea in &ticket.lineas {
        let neto = linea.neto_cents();
        let iva = linea.iva_cents();
        match por_tasa.iter_mut().find(|(t, _, _)| *t == linea.tasa) {
            Some((_, n, i)) => {
                *n += neto;
                *i += iva;
            }
            None => por_tasa.push((linea.tasa, neto, iva)),
        }
    }

    por_tasa
        .into_iter()
        .map(|(tasa, neto, iva)| Concepto {
            clave_prod_serv: CLAVE_SAT_GENERICA.to_string(),
            clave_unidad: CLAVE_UNIDAD_PIEZA.to_string(),
            unidad: "Pieza".to_string(),
            sku: ticket.folio.clone(),
            descripcion: format!("Venta del día según nota {}", ticket.folio),
            cantidad_millis: 1000,
            valor_unitario_cents: neto,
            descuento_cents: 0,
            tasa,
            // El IVA va dicho con número, no recalculado: es lo que hace que
            // el total de la factura sea idéntico al dinero que entró a la
            // caja. Recalcularlo aquí pierde un centavo en cuanto el importe
            // no reparte exacto entre 1.16.
            iva_cents: Some(iva),
        })
        .collect()
}

/// Arma la factura global de un periodo.
///
/// `periodicidad` viene del catálogo c_Periodicidad: 01 diario, 02 semanal,
/// 03 quincenal, 04 mensual, 05 bimestral. `meses` es el número del mes (o el
/// rango, "01-02" para bimestre) y `anio` el año con cuatro dígitos.
pub fn desde_tickets(
    emisor: &Emisor,
    tickets: &[Ticket],
    fecha: &str,
    forma_pago: &str,
    periodicidad: &str,
    meses: &str,
    anio: &str,
) -> Comprobante {
    let mut c = Comprobante::ingreso(
        "G",
        &folio_global(fecha, periodicidad),
        fecha,
        emisor.clone(),
        Receptor::publico_general(&emisor.codigo_postal),
    );
    c.forma_pago = forma_pago.to_string();
    c.como_global(periodicidad, meses, anio);
    for t in tickets {
        if t.lineas.is_empty() {
            continue;
        }
        for concepto in conceptos_de(t) {
            c.agregar(concepto);
        }
    }
    c
}

/// Folio sugerido: la fecha y la periodicidad, para que no se repita y se
/// pueda encontrar después ("G20261011-01").
pub fn folio_global(fecha: &str, periodicidad: &str) -> String {
    let dia: String = fecha
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(8)
        .collect();
    format!("{dia}-{periodicidad}")
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

    fn linea(precio: i64, tasa: TasaIva) -> LineaTicket {
        LineaTicket {
            sku: "7501".into(),
            descripcion: "Cosa".into(),
            cantidad_millis: 1000,
            precio_bruto_cents: precio,
            descuento_cents: 0,
            clave_sat: CLAVE_SAT_GENERICA.into(),
            clave_unidad: CLAVE_UNIDAD_PIEZA.into(),
            unidad: "Pieza".into(),
            tasa,
        }
    }

    #[test]
    fn el_iva_se_quita_y_el_importe_vuelve_a_cuadrar() {
        // $18.00 con IVA → $15.52 + $2.48 = $18.00 exactos.
        let l = linea(1800, TasaIva::Dieciseis);
        assert_eq!(l.neto_cents(), 1552);
        assert_eq!(l.iva_cents(), 248);
        assert_eq!(l.neto_cents() + l.iva_cents(), 1800);
    }

    #[test]
    fn el_iva_por_complemento_no_pierde_centavos_en_ningun_importe() {
        // Se prueban los primeros 20,000 importes: es más barato que una
        // auditoría y caza el redondeo que se come un centavo.
        for bruto in 1..20_000i64 {
            let l = linea(bruto, TasaIva::Dieciseis);
            assert_eq!(
                l.neto_cents() + l.iva_cents(),
                bruto,
                "no cuadró con {bruto} centavos"
            );
            // Y el IVA está a lo más un centavo del 16% real.
            let esperado = redondear(bruto * 16, 116);
            assert!(
                (l.iva_cents() - esperado).abs() <= 1,
                "IVA descuadrado en {bruto}: dio {} y esperaba {esperado}",
                l.iva_cents()
            );
        }
    }

    #[test]
    fn el_descuento_baja_la_base_y_el_iva() {
        let mut l = linea(1800, TasaIva::Dieciseis);
        l.descuento_cents = 200; // le bajaron $2.00
        assert_eq!(l.bruto_cents(), 1600);
        assert_eq!(l.total_cents(), 1600);
        // 16.00 = 13.79 + 2.21
        assert_eq!(l.neto_cents(), 1379);
        assert_eq!(l.iva_cents(), 221);
        assert_eq!(l.neto_cents() + l.iva_cents(), 1600);
    }

    #[test]
    fn las_cantidades_con_decimales_multiplican_bien() {
        let mut l = linea(1552, TasaIva::Dieciseis);
        l.cantidad_millis = 2500; // 2.500 kg a $15.52 sin IVA... bruto
        assert_eq!(l.bruto_cents(), 1552 * 2500 / 1000);
    }

    #[test]
    fn un_ticket_con_dos_tasas_sale_en_dos_renglones() {
        let t = Ticket {
            folio: "A-1".into(),
            lineas: vec![
                linea(1800, TasaIva::Dieciseis),
                linea(1800, TasaIva::Dieciseis), // se junta con la anterior
                linea(1000, TasaIva::Exento),
            ],
        };
        let c = conceptos_de(&t);
        assert_eq!(c.len(), 2, "las tasas iguales deben juntarse: {c:?}");
        assert_eq!(c[0].valor_unitario_cents, 3104); // 2 × 1552
        assert_eq!(c[1].valor_unitario_cents, 1000);
        assert_eq!(c[1].tasa, TasaIva::Exento);
    }

    #[test]
    fn la_factura_global_suma_exactamente_lo_que_se_cobró() {
        // Esta es la prueba que importa: lo que dice la factura tiene que ser
        // lo que dicen los tickets del día, centavo por centavo.
        let tickets = vec![
            Ticket {
                folio: "A-1".into(),
                lineas: vec![
                    linea(1800, TasaIva::Dieciseis),
                    linea(2320, TasaIva::Dieciseis),
                ],
            },
            Ticket {
                folio: "A-2".into(),
                lineas: vec![
                    {
                        let mut l = linea(5000, TasaIva::Dieciseis);
                        l.descuento_cents = 500;
                        l
                    },
                    linea(1250, TasaIva::Cero),
                ],
            },
            Ticket {
                folio: "A-3".into(),
                lineas: vec![linea(999, TasaIva::Dieciseis)],
            },
        ];
        let cobrado: i64 = tickets.iter().map(|t| t.total_cents()).sum();

        let c = desde_tickets(
            &emisor(),
            &tickets,
            "2026-10-11T23:59:00",
            "01",
            "01",
            "10",
            "2026",
        );
        assert_eq!(
            c.total_cents(),
            cobrado,
            "la factura dice {} pero se cobraron {cobrado}",
            c.total_cents()
        );
        // Y los datos de la factura global están completos.
        assert_eq!(c.receptor.rfc, "XAXX010101000");
        assert_eq!(c.receptor.uso_cfdi, "S01");
        assert_eq!(c.periodicidad.as_deref(), Some("01"));
        assert_eq!(c.meses.as_deref(), Some("10"));
        assert_eq!(c.anio.as_deref(), Some("2026"));
        // Tres tickets, pero el del 16% y el de tasa 0 se separan.
        assert_eq!(c.conceptos.len(), 4, "cada tasa va en su renglón");
    }

    #[test]
    fn juntar_renglones_en_uno_no_pierde_centavos() {
        // Éste es el bug que se encontró en la tienda: dos cosas de $18.00
        // juntas son $36.00, y como el precio trae el IVA incluido, $36.00 no
        // reparte exacto entre 1.16 ($31.03 + $4.96 = $35.99, $31.04 + $4.97
        // = $36.01). El IVA tiene que completar el importe cobrado, no
        // recalcularse: si no, la factura dice un centavo menos que la caja.
        let t = Ticket {
            folio: "A-1".into(),
            lineas: vec![
                linea(1800, TasaIva::Dieciseis),
                linea(1800, TasaIva::Dieciseis),
            ],
        };
        let c = desde_tickets(
            &emisor(),
            std::slice::from_ref(&t),
            "2026-10-11T23:59:00",
            "01",
            "01",
            "10",
            "2026",
        );
        assert_eq!(c.total_cents(), 3600, "la factura no dice lo que se cobró");
        assert_eq!(c.total_cents(), t.total_cents());
    }

    #[test]
    fn ningun_importe_ni_ninguna_mezcla_pierde_un_centavo() {
        // Fuerza bruta: es más barato que una auditoría y caza el redondeo que
        // sólo aparece con cierta combinación de importes.
        for importe in 1..400i64 {
            for lineas in [
                vec![linea(importe, TasaIva::Dieciseis)],
                vec![
                    linea(importe, TasaIva::Dieciseis),
                    linea(importe, TasaIva::Dieciseis),
                ],
                vec![
                    linea(importe, TasaIva::Dieciseis),
                    linea(1847, TasaIva::Dieciseis),
                ],
                vec![
                    {
                        let mut l = linea(importe, TasaIva::Dieciseis);
                        l.descuento_cents = importe / 7;
                        l
                    },
                    linea(2320, TasaIva::Dieciseis),
                ],
            ] {
                let t = Ticket {
                    folio: "A-1".into(),
                    lineas,
                };
                let cobrado = t.total_cents();
                let c = desde_tickets(
                    &emisor(),
                    std::slice::from_ref(&t),
                    "2026-10-11T23:59:00",
                    "01",
                    "01",
                    "10",
                    "2026",
                );
                assert_eq!(
                    c.total_cents(),
                    cobrado,
                    "con {importe} centavos la factura dice {} y se cobraron {cobrado}",
                    c.total_cents()
                );
            }
        }
    }

    #[test]
    fn una_factura_sin_ventas_no_se_genera() {
        let c = desde_tickets(
            &emisor(),
            &[],
            "2026-10-11T23:59:00",
            "01",
            "01",
            "10",
            "2026",
        );
        assert!(c.conceptos.is_empty());
        assert_eq!(c.total_cents(), 0);
    }

    #[test]
    fn el_follo_lleva_la_fecha_y_la_periodicidad() {
        assert_eq!(
            folio_global("2026-10-11T23:59:00", "01"),
            "20261011-01",
            "así se encuentra la factura de ese día después"
        );
    }

    #[test]
    fn el_xml_de_la_global_trae_todo_lo_que_pide_el_sat() {
        let tickets = vec![Ticket {
            folio: "A-77".into(),
            lineas: vec![linea(11600, TasaIva::Dieciseis)],
        }];
        let c = desde_tickets(
            &emisor(),
            &tickets,
            "2026-10-11T23:59:00",
            "01",
            "01",
            "10",
            "2026",
        );
        let x = crate::cfdi::xml(&c);
        assert!(x.contains("Periodicidad=\"01\""));
        assert!(x.contains("Meses=\"10\""));
        assert!(x.contains("Año=\"2026\""));
        assert!(x.contains("Rfc=\"XAXX010101000\""));
        assert!(
            x.contains("nota A-77"),
            "la descripción debe citar el ticket"
        );
        assert!(x.contains("Total=\"116.00\""), "{}", x);
    }
}

#[cfg(test)]
mod pruebas_nota {
    use super::*;
    use crate::cfdi::Tipo;

    fn emisor() -> Emisor {
        Emisor {
            rfc: "XAXX010101000".into(),
            nombre: "Abarrotes Lupita".into(),
            regimen_fiscal: "626".into(),
            codigo_postal: "91260".into(),
        }
    }

    fn linea(precio: i64) -> LineaTicket {
        LineaTicket {
            sku: "7501".into(),
            descripcion: "Cosa".into(),
            cantidad_millis: 1000,
            precio_bruto_cents: precio,
            descuento_cents: 0,
            clave_sat: CLAVE_SAT_GENERICA.into(),
            clave_unidad: CLAVE_UNIDAD_PIEZA.into(),
            unidad: "Pieza".into(),
            tasa: TasaIva::Dieciseis,
        }
    }

    fn devolucion(precios: &[i64]) -> Ticket {
        Ticket {
            folio: "A-1".into(),
            lineas: precios.iter().map(|p| linea(*p)).collect(),
        }
    }

    #[test]
    fn la_nota_es_de_egreso_y_va_ligada_a_la_factura_original() {
        let dev = devolucion(&[1800, 2320]);
        let c = nota_credito(
            &emisor(),
            &Receptor::publico_general("91260"),
            &dev,
            "2026-10-12T18:00:00",
            Relacion {
                tipo_relacion: "01".into(),
                uuids: vec!["AAAABBBB-1111-2222-3333-444455556666".into()],
            },
            "NC-1",
        );
        assert_eq!(c.tipo, Tipo::Egreso);
        let x = crate::cfdi::xml(&c);
        assert!(x.contains("TipoDeComprobante=\"E\""), "{}", x);
        assert!(
            x.contains("<cfdi:CfdiRelacionados TipoRelacion=\"01\">"),
            "{}",
            x
        );
        assert!(
            x.contains("UUID=\"AAAABBBB-1111-2222-3333-444455556666\""),
            "sin la factura original la nota no sirve"
        );
        // Un egreso no lleva forma ni método de pago.
        assert!(!x.contains("FormaPago"), "{}", x);
        assert!(!x.contains("MetodoPago"), "{}", x);
    }

    #[test]
    fn la_nota_devuelve_exactamente_lo_que_se_devolvio() {
        let dev = devolucion(&[1800, 2320]);
        let c = nota_credito(
            &emisor(),
            &Receptor::publico_general("91260"),
            &dev,
            "2026-10-12T18:00:00",
            Relacion {
                tipo_relacion: "01".into(),
                uuids: vec!["ABC".into()],
            },
            "NC-1",
        );
        assert_eq!(c.total_cents(), 1800 + 2320, "la nota no dice lo devuelto");
        assert_eq!(c.total_cents(), dev.total_cents());
    }

    /// Cinco cocas y devuelven dos: la nota dice dos, no cinco.
    #[test]
    fn la_nota_parcial_solo_cubre_lo_que_se_devuelve() {
        let vendida = LineaTicket {
            cantidad_millis: 5000,
            ..linea(1800)
        };
        let devuelta = vendida.parcial(2000);
        let t = Ticket {
            folio: "A-1".into(),
            lineas: vec![devuelta],
        };
        assert_eq!(
            t.total_cents(),
            1800 * 2,
            "la nota cubre dos cocas, no cinco"
        );
        let c = nota_credito(
            &emisor(),
            &Receptor::publico_general("91260"),
            &t,
            "2026-10-12T18:00:00",
            Relacion {
                tipo_relacion: "01".into(),
                uuids: vec!["ABC".into()],
            },
            "NC-1",
        );
        assert_eq!(c.total_cents(), 3600);
    }

    /// Si a cinco cosas les quitaste $2.00, a dos se les quitan 80 centavos.
    #[test]
    fn el_descuento_se_reparte_proporcional() {
        let vendida = LineaTicket {
            cantidad_millis: 5000,
            descuento_cents: 200,
            ..linea(1800)
        };
        let devuelta = vendida.parcial(2000);
        assert_eq!(
            devuelta.descuento_cents, 80,
            "dos de cinco llevan dos quintos del descuento"
        );
        // 2 × $18.00 = $36.00, menos $0.80 = $35.20
        assert_eq!(devuelta.bruto_cents(), 3600 - 80);
    }

    /// Devolver más de lo que se vendió es un error de dedo, no un crédito.
    #[test]
    fn nadie_devuelve_mas_de_lo_que_compro() {
        let vendida = LineaTicket {
            cantidad_millis: 5000,
            ..linea(1800)
        };
        let devuelta = vendida.parcial(90_000);
        assert_eq!(
            devuelta.cantidad_millis, 5000,
            "se topa en lo que se vendió"
        );
        let negativa = vendida.parcial(-1000);
        assert_eq!(negativa.cantidad_millis, 0, "no hay devoluciones negativas");
    }

    /// La nota dice QUÉ se devolvió: dos cocas, no "venta del día".
    #[test]
    fn la_nota_dice_que_se_devolvio() {
        let t = Ticket {
            folio: "A-1".into(),
            lineas: vec![
                LineaTicket {
                    cantidad_millis: 2000,
                    descripcion: "Coca-Cola 600 ml".into(),
                    ..linea(1800)
                },
                LineaTicket {
                    sku: "7502".into(),
                    descripcion: "Pan blanco".into(),
                    cantidad_millis: 1500,
                    ..linea(1850)
                },
            ],
        };
        let c = nota_credito(
            &emisor(),
            &Receptor::publico_general("91260"),
            &t,
            "2026-10-12T18:00:00",
            Relacion {
                tipo_relacion: "01".into(),
                uuids: vec!["ABC".into()],
            },
            "NC-1",
        );
        assert_eq!(c.conceptos.len(), 2, "un renglón por cosa devuelta");
        let x = crate::cfdi::xml(&c);
        assert!(x.contains("2 x Coca-Cola 600 ml"), "{}", x);
        assert!(x.contains("1.5 x Pan blanco"), "{}", x);
        // Y el total es exacto: 2 × $18.00 + 1.5 × $18.50 = $63.75
        assert_eq!(c.total_cents(), 3600 + 2775);
    }

    #[test]
    fn sin_factura_original_la_nota_no_finge_que_la_tiene() {
        let dev = devolucion(&[1800]);
        let c = nota_credito(
            &emisor(),
            &Receptor::publico_general("91260"),
            &dev,
            "2026-10-12T18:00:00",
            Relacion {
                tipo_relacion: "03".into(),
                uuids: vec![],
            },
            "NC-1",
        );
        let x = crate::cfdi::xml(&c);
        assert!(
            !x.contains("CfdiRelacionados"),
            "sin UUID no se inventa un nodo vacío: {x}"
        );
    }

    #[test]
    fn una_venta_normal_sigue_siendo_de_ingreso_y_con_forma_de_pago() {
        let t = devolucion(&[1800]);
        let c = desde_tickets(
            &emisor(),
            std::slice::from_ref(&t),
            "2026-10-11T23:59:00",
            "01",
            "01",
            "10",
            "2026",
        );
        let x = crate::cfdi::xml(&c);
        assert!(x.contains("TipoDeComprobante=\"I\""), "{}", x);
        assert!(x.contains("FormaPago=\"01\""), "{}", x);
        assert!(x.contains("MetodoPago=\"PUE\""), "{}", x);
    }
}
