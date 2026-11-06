//! Facturación electrónica (CFDI 4.0) en la interfaz.
//!
//! Vive dentro de Configuración y no como pantalla aparte por dos razones: la
//! barra de arriba ya no cabe, y facturar es algo que se hace una vez al día,
//! no cada cinco minutos.
//!
//! Todo lo que se ve aquí pasa por la misma API que usa el móvil: la interfaz
//! no calcula impuestos ni arma XML, sólo pide y muestra.

use eframe::egui;
use pos_api::{Comando, Cuerpo, LineaDevolucionDto, Peticion, Respuesta};
use pos_core::Money;

use crate::{theme, PosApp};

/// Versión que se muestra al usuario. La misma que usa el instalador.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// La licencia completa, tal cual, para que se pueda leer sin salir del
/// programa. Es la MIT: corta, permisiva y sin sorpresas.
const TEXTO_LICENCIA: &str = "Licencia MIT\n\
\n\
Copyright (c) 2026 Equipo del punto de venta\n\
\n\
Por la presente se concede permiso, de forma gratuita, a cualquier persona que\
obtenga una copia de este software y de los archivos de documentación\
asociados, a usar el software sin restricción, incluidos los derechos de usar,\
copiar, modificar, fusionar, publicar, distribuir, sublicenciar y/o vender\
copias del mismo, siempre que se incluya el aviso de copyright y este aviso de\
permiso en todas las copias o partes sustanciales del software.\n\
\n\
EL SOFTWARE SE PROPORCIONA \"TAL CUAL\", SIN GARANTÍA DE NINGÚN TIPO, EXPRESA O\
IMPLÍCITA, INCLUYENDO PERO NO LIMITADO A GARANTÍAS DE COMERCIABILIDAD,\
IDONEIDAD PARA UN PROPÓSITO PARTICULAR Y NO INFRACCIÓN. EN NINGÚN CASO LOS\
AUTORES O TITULARES DEL COPYRIGHT SERÁN RESPONSABLES DE NINGUNA RECLAMACIÓN,\
DAÑO U OTRA RESPONSABILIDAD.";

/// Formas de pago que usa una tienda (catálogo `c_FormaPago`).
const FORMAS_PAGO: [(&str, &str); 5] = [
    ("01", "Efectivo"),
    ("04", "Tarjeta de credito"),
    ("28", "Tarjeta de debito"),
    ("03", "Transferencia"),
    ("99", "Otros"),
];

/// Motivos de cancelación SAT (`c_MotivoCancelacion`).
const MOTIVOS_CANCELACION: [(&str, &str); 4] = [
    ("01", "01 Errores con relación (lleva UUID que sustituye)"),
    ("02", "02 Errores sin relación"),
    ("03", "03 No se llevó a cabo la operación"),
    ("04", "04 Nominativa relacionada en una factura global"),
];

/// Periodicidad de la factura global (catálogo `c_Periodicidad`).
const PERIODICIDADES: [(&str, &str); 5] = [
    ("01", "Diaria"),
    ("02", "Semanal"),
    ("03", "Quincenal"),
    ("04", "Mensual"),
    ("05", "Bimestral"),
];

fn nombre_de<'a>(lista: &'a [(&'a str, &'a str)], clave: &str) -> &'a str {
    lista
        .iter()
        .find(|(c, _)| *c == clave)
        .map(|(_, n)| *n)
        .unwrap_or("Otros")
}

fn pesos(cents: i64) -> String {
    Money::from_cents(cents).format_mxn()
}

impl PosApp {
    /// Carga los datos fiscales y la cola de facturas.
    pub fn refrescar_fiscal(&mut self) {
        if let Respuesta::Ok(Cuerpo::ConfigFiscal(c)) = self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::ConfigFiscal,
        }) {
            self.fiscal = *c;
        }
        if let Respuesta::Ok(Cuerpo::Facturas(v)) = self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::Facturas { limite: 200 },
        }) {
            self.facturas = v;
        }
        if self.factura_fecha.trim().is_empty() {
            self.factura_fecha = crate::app::ahora_cdmx().chars().take(10).collect();
        }
        if self.nota_fecha.trim().is_empty() {
            self.nota_fecha = crate::app::ahora_cdmx().chars().take(10).collect();
        }
        if self.factura_individual_fecha.trim().is_empty() {
            self.factura_individual_fecha = crate::app::ahora_cdmx().chars().take(10).collect();
        }
        // Las ventas con RFC no entran en la global: si nadie las factura,
        // ese ingreso se queda sin reportar. Hay que decirlo.
        if let Respuesta::Ok(Cuerpo::Texto(n)) = self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::VentasPorFacturar,
        }) {
            self.pendientes_rfc = n.parse().unwrap_or(0);
        }
    }

    /// Factura una venta a nombre del cliente que la compró.
    fn facturar_venta_individual(&mut self) {
        let folio = self.factura_individual_folio.trim().to_string();
        let fecha = self.factura_individual_fecha.trim().to_string();
        if folio.is_empty() {
            self.mensaje = "Escribe el folio de la venta a facturar.".into();
            return;
        }
        if fecha.len() != 10 {
            self.mensaje = "Escribe el día de la factura como 2026-10-12.".into();
            return;
        }
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::FacturarVenta { folio, fecha },
        }) {
            Respuesta::Ok(Cuerpo::Factura(f)) => {
                self.mensaje = format!(
                    "Factura {} para {} por {} lista. Quedó pendiente de timbre.",
                    f.folio,
                    f.receptor_rfc,
                    pesos(f.total_cents)
                );
                self.factura_individual_folio.clear();
                self.refrescar_fiscal();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada".into(),
        }
    }

    /// La sección completa, tal como se ve en Configuración.
    pub(crate) fn seccion_facturacion(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        egui::CollapsingHeader::new(
            egui::RichText::new("Facturación electrónica (CFDI 4.0)").heading(),
        )
        .default_open(false)
        .show(ui, |ui| {
            if self.fiscal.rfc.trim().is_empty() {
                ui.colored_label(
                    theme::advertencia(),
                    "Sin los datos fiscales no se puede facturar. Llénalos una vez y listo.",
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&self.fiscal.rfc).strong());
                    ui.label(&self.fiscal.nombre);
                    ui.colored_label(
                        theme::texto_suave(),
                        format!("CP {}", self.fiscal.codigo_postal),
                    );
                });
            }
            ui.separator();

            // --- datos fiscales ---
            egui::Grid::new("datos_fiscales")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("RFC");
                    ui.add_sized(
                        [200.0, 26.0],
                        egui::TextEdit::singleline(&mut self.fiscal.rfc).hint_text("XAXX010101000"),
                    );
                    ui.end_row();

                    ui.label("Nombre o razón social");
                    ui.add_sized(
                        [380.0, 26.0],
                        egui::TextEdit::singleline(&mut self.fiscal.nombre)
                            .hint_text("Como aparece en tu constancia fiscal"),
                    );
                    ui.end_row();

                    ui.label("Regimen fiscal");
                    ui.add_sized(
                        [120.0, 26.0],
                        egui::TextEdit::singleline(&mut self.fiscal.regimen_fiscal)
                            .hint_text("626"),
                    );
                    ui.end_row();

                    ui.label("Código postal");
                    ui.add_sized(
                        [120.0, 26.0],
                        egui::TextEdit::singleline(&mut self.fiscal.codigo_postal)
                            .hint_text("91260"),
                    );
                    ui.end_row();

                    ui.label("Forma de pago");
                    egui::ComboBox::from_id_salt("forma_pago")
                        .selected_text(nombre_de(&FORMAS_PAGO, &self.fiscal.forma_pago))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (clave, texto) in FORMAS_PAGO {
                                ui.selectable_value(
                                    &mut self.fiscal.forma_pago,
                                    clave.to_string(),
                                    texto,
                                );
                            }
                        });
                    ui.end_row();

                    ui.label("Periodicidad");
                    egui::ComboBox::from_id_salt("periodicidad")
                        .selected_text(nombre_de(&PERIODICIDADES, &self.fiscal.periodicidad))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (clave, texto) in PERIODICIDADES {
                                ui.selectable_value(
                                    &mut self.fiscal.periodicidad,
                                    clave.to_string(),
                                    texto,
                                );
                            }
                        });
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.collapsing("Carpetas: el contador timbra, el programa no sale a internet", |ui| {
                ui.label(
                    "Tres pasos, sin PAC por HTTP: 1) el programa deja el XML en «Por timbrar»; \
                     2) el contador lo timbra por fuera; 3) el XML timbrado se recoge de «Timbrados».",
                );
                ui.horizontal(|ui| {
                    ui.label("Por timbrar");
                    ui.add_sized(
                        [320.0, 26.0],
                        egui::TextEdit::singleline(&mut self.fiscal.carpeta_salida)
                            .hint_text("C:\\Facturas\\por-timbrar"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Timbrados");
                    ui.add_sized(
                        [320.0, 26.0],
                        egui::TextEdit::singleline(&mut self.fiscal.carpeta_entrada)
                            .hint_text("C:\\Facturas\\timbrados"),
                    );
                });
                let salida = self.fiscal.carpeta_salida.trim().to_ascii_lowercase();
                let entrada = self.fiscal.carpeta_entrada.trim().to_ascii_lowercase();
                if salida.starts_with("http://")
                    || salida.starts_with("https://")
                    || entrada.starts_with("http://")
                    || entrada.starts_with("https://")
                {
                    ui.colored_label(
                        theme::peligro(),
                        "Eso es una dirección de internet. El timbrado por HTTP está bloqueado: usa una carpeta.",
                    );
                }
            });

            ui.horizontal(|ui| {
                if ui.button("Guardar datos fiscales").clicked() {
                    self.guardar_fiscal();
                }
                if ui.button("Recargar").clicked() {
                    self.refrescar_fiscal();
                }
            });

            // --- generar la factura global ---
            ui.add_space(8.0);
            ui.separator();
            ui.label(
                egui::RichText::new("Factura global")
                    .strong()
                    .color(theme::acento()),
            );
            ui.label("Junta en un solo comprobante las ventas del día que nadie pidió facturar.");
            ui.horizontal(|ui| {
                ui.label("Día");
                ui.add_sized(
                    [140.0, 26.0],
                    egui::TextEdit::singleline(&mut self.factura_fecha).hint_text("2026-10-11"),
                );
                ui.label("Periodicidad");
                egui::ComboBox::from_id_salt("periodicidad_global")
                    .selected_text(nombre_de(&PERIODICIDADES, &self.factura_periodicidad))
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        for (clave, texto) in PERIODICIDADES {
                            ui.selectable_value(
                                &mut self.factura_periodicidad,
                                clave.to_string(),
                                texto,
                            );
                        }
                    });
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("Generar factura global").color(theme::acento()),
                    ))
                    .clicked()
                {
                    self.generar_factura_global();
                }
            });

            // --- factura individual: la que se le da a quien la pide ---
            ui.add_space(6.0);
            ui.separator();
            ui.label(
                egui::RichText::new("Factura de una venta (cliente con RFC)")
                    .strong()
                    .color(theme::acento()),
            );
            if self.pendientes_rfc > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "Hay {} venta(s) de clientes con RFC esperando factura. Esas no \
                         entran en la factura global del día: si nadie las factura, ese \
                         ingreso se queda sin reportar.",
                        self.pendientes_rfc
                    ))
                    .color(theme::advertencia()),
                );
            }
            ui.horizontal(|ui| {
                ui.label("Venta");
                ui.add_sized(
                    [140.0, 26.0],
                    egui::TextEdit::singleline(&mut self.factura_individual_folio)
                        .hint_text("A-123"),
                );
                ui.label("Dia");
                ui.add_sized(
                    [120.0, 26.0],
                    egui::TextEdit::singleline(&mut self.factura_individual_fecha)
                        .hint_text("2026-10-12"),
                );
                if ui.button("Facturar a nombre del cliente").clicked() {
                    self.facturar_venta_individual();
                }
            });

            // --- nota de crédito ---
            ui.add_space(6.0);
            ui.separator();
            ui.label(
                egui::RichText::new("Nota de crédito (devolución)")
                    .strong()
                    .color(theme::acento()),
            );
            ui.label(
                "Para una venta ya facturada. La nota queda ligada a esa factura, que es \
                 como el SAT sabe de qué se está restando.",
            );
            ui.horizontal(|ui| {
                ui.label("Venta");
                ui.add_sized(
                    [140.0, 26.0],
                    egui::TextEdit::singleline(&mut self.nota_folio).hint_text("A-123"),
                );
                ui.label("Dia");
                ui.add_sized(
                    [120.0, 26.0],
                    egui::TextEdit::singleline(&mut self.nota_fecha).hint_text("2026-10-12"),
                );
                if ui.button("Buscar venta").clicked() {
                    self.buscar_venta_para_nota();
                }
                if ui.button("Hacer nota de crédito").clicked() {
                    self.hacer_nota_credito();
                }
            });

            // Los renglones de la venta, para devolver sólo una parte.
            if !self.nota_renglones.is_empty() {
                ui.add_space(4.0);
                ui.label("Cuánto se devuelve de cada cosa (vacío o 0 = no se devuelve):");
                for i in 0..self.nota_renglones.len() {
                    let (sku, nombre, vendida, _) = self.nota_renglones[i].clone();
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [110.0, 24.0],
                            egui::Label::new(egui::RichText::new(&sku).monospace()),
                        );
                        ui.add_sized(
                            [210.0, 24.0],
                            egui::Label::new(egui::RichText::new(&nombre).strong()),
                        );
                        ui.label(format!("vendido {}", Self::cantidad_corta(vendida)));
                        ui.add_sized(
                            [90.0, 24.0],
                            egui::TextEdit::singleline(&mut self.nota_renglones[i].3)
                                .hint_text("0"),
                        );
                    });
                }
                ui.label("Si todos quedan en cero o en blanco, la nota cubre la venta completa.");
            }
            ui.horizontal(|ui| {
                ui.label("O devolver un monto");
                ui.add_sized(
                    [110.0, 24.0],
                    egui::TextEdit::singleline(&mut self.nota_monto).hint_text("0.00"),
                );
                if ui.button("Nota por monto").clicked() {
                    self.hacer_nota_credito_monto();
                }
                ui.label(
                    egui::RichText::new("Para ropa y a granel, donde no hay piezas que contar.")
                        .weak()
                        .color(theme::texto_suave()),
                );
            });

            // --- cobranza: el complemento de pagos ---
            ui.add_space(6.0);
            ui.separator();
            ui.label(
                egui::RichText::new("Cobranza (complemento de pagos)")
                    .strong()
                    .color(theme::acento()),
            );
            ui.label(
                "Cuando le facturaste a crédito, la factura queda a deber. Cada pago que \
                 te hace va en un comprobante aparte, ligado a la factura que cubre: sin \
                 él, el cliente tiene una factura que no puede deducir.",
            );
            ui.horizontal(|ui| {
                ui.label("Cliente");
                let clientes = self.clientes.clone();
                let elegido = clientes
                    .iter()
                    .find(|c| c.id == self.pago_cliente_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "Elige al cliente que paga".to_string());
                egui::ComboBox::from_id_salt("cliente_que_paga")
                    .selected_text(elegido)
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        for c in clientes {
                            // Sólo quien tiene RFC puede recibir un complemento:
                            // los demás nunca tuvieron factura a su nombre.
                            let rfc = c.rfc.trim().to_uppercase();
                            if rfc.is_empty() || rfc == "XAXX010101000" {
                                continue;
                            }
                            ui.selectable_value(&mut self.pago_cliente_id, c.id, c.name.clone());
                        }
                    });
                if ui.button("Cuánto debe").clicked() {
                    self.consultar_saldo_pago();
                }
            });
            if self.pago_saldo_cents > 0 {
                ui.colored_label(
                    theme::advertencia(),
                    format!(
                        "Lleva debiendo {} en facturas a crédito.",
                        pesos(self.pago_saldo_cents)
                    ),
                );
            }
            ui.horizontal(|ui| {
                ui.label("Pagó");
                ui.add_sized(
                    [110.0, 24.0],
                    egui::TextEdit::singleline(&mut self.pago_monto).hint_text("0.00"),
                );
                ui.label("Dia");
                ui.add_sized(
                    [120.0, 24.0],
                    egui::TextEdit::singleline(&mut self.pago_fecha).hint_text("2026-10-12"),
                );
                ui.label("Como");
                egui::ComboBox::from_id_salt("forma_pago_cobranza")
                    .selected_text(nombre_de(&FORMAS_PAGO, &self.pago_forma))
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for (clave, texto) in FORMAS_PAGO {
                            ui.selectable_value(&mut self.pago_forma, clave.to_string(), texto);
                        }
                    });
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("Registrar pago").color(theme::acento()),
                    ))
                    .clicked()
                {
                    self.registrar_pago_credito();
                }
            });

            // --- la cola ---
            ui.add_space(8.0);
            let pendientes = self
                .facturas
                .iter()
                .filter(|f| f.estado == "pendiente")
                .count();
            if pendientes > 0 {
                ui.colored_label(
                    theme::advertencia(),
                    format!("{pendientes} factura(s) esperando timbre."),
                );
            }
            self.tabla_facturas(ui);
            self.forma_cancelar_cfdi(ui);

            // --- el XML, si se pidió ver uno ---
            if let Some((folio, xml)) = self.factura_xml.clone() {
                ui.add_space(6.0);
                ui.collapsing(format!("XML de {folio}"), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("xml_factura")
                        .max_height(260.0)
                        .show(ui, |ui| {
                            ui.monospace(xml.as_str());
                        });
                    if ui.button("Cerrar").clicked() {
                        self.factura_xml = None;
                    }
                });
            }
        });
    }

    /// La cola de facturas: folio, día, total, estado y qué se puede hacer.
    fn tabla_facturas(&mut self, ui: &mut egui::Ui) {
        if self.facturas.is_empty() {
            ui.colored_label(theme::texto_suave(), "Todavía no hay facturas.");
            return;
        }
        let mut reintentar: Option<i64> = None;
        let mut ver: Option<i64> = None;
        let mut cancelar: Option<i64> = None;

        egui::Grid::new("cola_facturas")
            .num_columns(7)
            .striped(true)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for f in &self.facturas {
                    ui.label(egui::RichText::new(&f.folio).strong());
                    ui.label(&f.periodo);
                    ui.label(pesos(f.total_cents));
                    let (texto, color) = match f.estado.as_str() {
                        "timbrada" => ("Timbrada", theme::exito()),
                        "error" => ("Rechazada", theme::peligro()),
                        "cancelada" => ("Cancelada", theme::peligro()),
                        _ => ("Pendiente", theme::advertencia()),
                    };
                    ui.colored_label(color, texto);
                    ui.label(
                        f.uuid
                            .clone()
                            .unwrap_or_else(|| f.ultimo_error.clone().unwrap_or_default()),
                    );
                    if f.estado != "timbrada" && f.estado != "cancelada" {
                        if ui.button("Reintentar").clicked() {
                            reintentar = Some(f.id);
                        }
                    } else {
                        ui.label("");
                    }
                    if f.estado != "cancelada" {
                        if ui.button("Cancelar CFDI").clicked() {
                            cancelar = Some(f.id);
                        }
                    } else {
                        ui.label("");
                    }
                    if ui.button("XML").clicked() {
                        ver = Some(f.id);
                    }
                    ui.end_row();
                }
            });

        if let Some(id) = reintentar {
            self.reintentar_factura(id);
        }
        if let Some(id) = ver {
            self.ver_xml_factura(id);
        }
        if let Some(id) = cancelar {
            self.cancel_factura_id = Some(id);
            self.cancel_motivo = "02".into();
            self.cancel_sustitucion.clear();
        }
    }

    fn forma_cancelar_cfdi(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.cancel_factura_id else {
            return;
        };
        let folio = self
            .facturas
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.folio.clone())
            .unwrap_or_default();
        let timbrada = self
            .facturas
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.estado == "timbrada")
            .unwrap_or(false);
        ui.add_space(8.0);
        ui.separator();
        ui.label(
            egui::RichText::new(format!("Cancelar {folio}"))
                .strong()
                .color(theme::peligro()),
        );
        ui.label(
            if timbrada {
                "Se deja la solicitud en «Por timbrar» para el contador (motivo SAT y UUID). El programa no habla con el SAT por internet."
            } else {
                "Todavía no se timbra: se baja de la cola. No hay solicitud para el SAT."
            },
        );
        ui.horizontal(|ui| {
            ui.label("Motivo");
            let texto = MOTIVOS_CANCELACION
                .iter()
                .find(|(c, _)| *c == self.cancel_motivo)
                .map(|(_, t)| *t)
                .unwrap_or("02 Errores sin relación");
            egui::ComboBox::from_id_salt("motivo_cancel_cfdi")
                .selected_text(texto)
                .width(360.0)
                .show_ui(ui, |ui| {
                    for (clave, t) in MOTIVOS_CANCELACION {
                        ui.selectable_value(&mut self.cancel_motivo, clave.to_string(), t);
                    }
                });
        });
        if self.cancel_motivo == "01" {
            ui.horizontal(|ui| {
                ui.label("UUID que sustituye");
                ui.add_sized(
                    [320.0, 26.0],
                    egui::TextEdit::singleline(&mut self.cancel_sustitucion)
                        .hint_text("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"),
                );
            });
        }
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("Confirmar cancelación").color(theme::peligro()),
                ))
                .clicked()
            {
                self.confirmar_cancelar_cfdi();
            }
            if ui.button("No cancelar").clicked() {
                self.cancel_factura_id = None;
            }
        });
    }

    fn confirmar_cancelar_cfdi(&mut self) {
        let Some(id) = self.cancel_factura_id else {
            return;
        };
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::CancelarFactura {
                id,
                motivo: self.cancel_motivo.clone(),
                uuid_sustitucion: self.cancel_sustitucion.clone(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Factura(f)) => {
                self.mensaje = format!("Factura {} cancelada.", f.folio);
                self.cancel_factura_id = None;
                self.refrescar_fiscal();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada al cancelar.".into(),
        }
    }

    fn guardar_fiscal(&mut self) {
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::GuardarConfigFiscal {
                config: self.fiscal.clone(),
            },
        }) {
            Respuesta::Ok(_) => {
                self.mensaje = "Datos fiscales guardados.".into();
                self.refrescar_fiscal();
            }
            Respuesta::Error(e) => self.mensaje = e,
        }
    }

    fn generar_factura_global(&mut self) {
        let fecha = self.factura_fecha.trim().to_string();
        if fecha.len() != 10 {
            self.mensaje = "Escribe el día como 2026-10-11.".into();
            return;
        }
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::FacturaGlobal {
                fecha,
                periodicidad: self.factura_periodicidad.clone(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Factura(f)) => {
                self.mensaje = format!(
                    "Factura {} por {} lista. {}",
                    f.folio,
                    pesos(f.total_cents),
                    if f.estado == "timbrada" {
                        "Timbrada.".to_string()
                    } else {
                        "Quedó pendiente de timbre.".to_string()
                    }
                );
                self.refrescar_fiscal();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada al facturar".into(),
        }
    }

    /// "2" -> 2000, "1.5" -> 1500. Vacío o negativo -> nada.
    fn millis_de(texto: &str) -> Option<i64> {
        let t = texto.trim().replace(',', ".");
        if t.is_empty() {
            return None;
        }
        let n: f64 = t.parse().ok()?;
        if n <= 0.0 {
            return None;
        }
        Some((n * 1000.0).round() as i64)
    }

    /// 2000 -> "2", 1500 -> "1.5".
    fn cantidad_corta(millis: i64) -> String {
        if millis % 1000 == 0 {
            format!("{}", millis / 1000)
        } else {
            format!("{:.3}", millis as f64 / 1000.0)
                .trim_end_matches('0')
                .to_string()
        }
    }

    /// De quién es esto y con qué permiso se usa.
    ///
    /// Está a la vista porque quien compra o usa un programa tiene derecho a
    /// saber qué puede hacer con él, sin tener que leer un archivo aparte.
    pub fn seccion_acerca_de(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.separator();
        ui.label(
            egui::RichText::new("Acerca de")
                .strong()
                .color(theme::acento()),
        );
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Punto de venta").strong());
            ui.label(
                egui::RichText::new(format!("versión {VERSION}"))
                    .monospace()
                    .color(theme::texto_suave()),
            );
            ui.label("·");
            ui.label("Licencia MIT");
        });
        ui.label(
            "Se puede usar, copiar, cambiar, mezclar, publicar, distribuir y vender, \
             incluso en un negocio, siempre que se conserve este aviso. Se entrega \
             como está, sin ninguna garantía.",
        );
        egui::CollapsingHeader::new("Ver la licencia completa")
            .id_salt("licencia")
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(TEXTO_LICENCIA)
                        .monospace()
                        .color(theme::texto_suave()),
                );
            });
        egui::CollapsingHeader::new("Licencias de las piezas de terceros")
            .id_salt("terceros")
            .show(ui, |ui| {
                ui.label(
                    "Este programa se construye sobre bibliotecas libres de Rust. Las \
                     principales son eframe/egui (la interfaz), rusqlite (la base de \
                     datos) y serde (el intercambio de datos con el móvil). Todas se \
                     distribuyen bajo licencias permisivas (MIT o Apache-2.0) que \
                     permiten usarlas en un producto comercial.",
                );
            });
    }

    /// Trae los renglones de la venta para poder devolver sólo una parte.
    fn buscar_venta_para_nota(&mut self) {
        let folio = self.nota_folio.trim().to_string();
        if folio.is_empty() {
            self.mensaje = "Escribe el folio de la venta.".into();
            return;
        }
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::TicketPorFolio {
                folio: folio.clone(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Ticket(t)) => {
                if t.lineas.is_empty() {
                    self.mensaje = format!("La venta {folio} no tiene renglones.");
                    return;
                }
                self.nota_renglones = t
                    .lineas
                    .iter()
                    .map(|l| {
                        (
                            l.sku.clone(),
                            l.descripcion.clone(),
                            l.qty_millis,
                            String::new(),
                        )
                    })
                    .collect();
                self.mensaje = format!(
                    "Venta {folio}: {} renglones. Anota cuánto se devuelve de cada uno.",
                    t.lineas.len()
                );
            }
            Respuesta::Error(e) => {
                self.nota_renglones.clear();
                self.mensaje = e;
            }
            _ => self.mensaje = "Respuesta inesperada".into(),
        }
    }

    /// Devuelve dinero, no piezas: "te devuelvo $50 de lo que compraste".
    fn hacer_nota_credito_monto(&mut self) {
        let folio = self.nota_folio.trim().to_string();
        let fecha = self.nota_fecha.trim().to_string();
        if folio.is_empty() {
            self.mensaje = "Escribe el folio de la venta que se devolvió.".into();
            return;
        }
        if fecha.len() != 10 {
            self.mensaje = "Escribe el día de la nota como 2026-10-12.".into();
            return;
        }
        let Some(monto) = Self::centavos_de(&self.nota_monto) else {
            self.mensaje = "Escribe el monto a devolver, por ejemplo 50.00.".into();
            return;
        };
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::NotaCreditoMonto {
                folio,
                fecha,
                monto_cents: monto,
            },
        }) {
            Respuesta::Ok(Cuerpo::Factura(f)) => {
                let cubierto = pesos(f.total_cents);
                // Si las piezas no repartieron exacto, se dice: un centavo
                // declarado es mejor que uno que nadie vio.
                let aviso = if f.total_cents != monto {
                    format!(
                        " Pediste {} y cubrió {} porque las piezas no reparten exacto.",
                        pesos(monto),
                        cubierto
                    )
                } else {
                    String::new()
                };
                self.mensaje = format!(
                    "Nota {} por {} lista. Quedó pendiente de timbre.{}",
                    f.folio, cubierto, aviso
                );
                self.nota_folio.clear();
                self.nota_monto.clear();
                self.nota_renglones.clear();
                self.refrescar_fiscal();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada".into(),
        }
    }

    /// Pregunta cuánto sigue debiendo el cliente elegido.
    fn consultar_saldo_pago(&mut self) {
        if self.pago_cliente_id <= 0 {
            self.mensaje = "Elige primero al cliente que está pagando.".into();
            return;
        }
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::SaldoPorPagar {
                cliente_id: self.pago_cliente_id,
            },
        }) {
            Respuesta::Ok(Cuerpo::Texto(t)) => {
                self.pago_saldo_cents = t.trim().parse().unwrap_or(0);
                self.mensaje = if self.pago_saldo_cents > 0 {
                    format!("Debe {}.", pesos(self.pago_saldo_cents))
                } else {
                    "Ese cliente no debe facturas a crédito.".into()
                };
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada".into(),
        }
    }

    /// Emite el complemento de pagos del dinero que acaba de entrar.
    fn registrar_pago_credito(&mut self) {
        if self.pago_cliente_id <= 0 {
            self.mensaje = "Elige primero al cliente que está pagando.".into();
            return;
        }
        let fecha = self.pago_fecha.trim().to_string();
        if fecha.len() != 10 {
            self.mensaje = "Escribe el día del pago como 2026-10-12.".into();
            return;
        }
        let Some(monto) = Self::centavos_de(&self.pago_monto) else {
            self.mensaje = "Escribe cuánto pagó, por ejemplo 500.00.".into();
            return;
        };
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::ComplementoPago {
                cliente_id: self.pago_cliente_id,
                monto_cents: monto,
                fecha,
                forma_pago: self.pago_forma.clone(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Factura(f)) => {
                self.mensaje = format!(
                    "Complemento {} por {} listo. Quedó pendiente de timbre.",
                    f.folio,
                    pesos(monto)
                );
                self.pago_monto.clear();
                self.pago_saldo_cents = 0;
                self.refrescar_fiscal();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada".into(),
        }
    }

    /// "50.00" -> 5000 centavos. `None` si no es un número.
    fn centavos_de(texto: &str) -> Option<i64> {
        let t = texto.trim().replace(',', ".");
        if t.is_empty() {
            return None;
        }
        let n: f64 = t.parse().ok()?;
        if n <= 0.0 {
            return None;
        }
        Some((n * 100.0).round() as i64)
    }

    fn hacer_nota_credito(&mut self) {
        let folio = self.nota_folio.trim().to_string();
        let fecha = self.nota_fecha.trim().to_string();
        if folio.is_empty() {
            self.mensaje = "Escribe el folio de la venta que se devolvió.".into();
            return;
        }
        if fecha.len() != 10 {
            self.mensaje = "Escribe el día de la nota como 2026-10-12.".into();
            return;
        }
        let lineas: Vec<LineaDevolucionDto> = self
            .nota_renglones
            .iter()
            .filter_map(|(sku, _, _, texto)| {
                Self::millis_de(texto).map(|cantidad| LineaDevolucionDto {
                    sku: sku.clone(),
                    cantidad_millis: cantidad,
                })
            })
            .collect();
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::NotaCredito {
                folio,
                fecha,
                lineas,
            },
        }) {
            Respuesta::Ok(Cuerpo::Factura(f)) => {
                self.mensaje = format!(
                    "Nota {} por {} lista. Quedó pendiente de timbre.",
                    f.folio,
                    pesos(f.total_cents)
                );
                self.nota_folio.clear();
                self.nota_renglones.clear();
                self.refrescar_fiscal();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada".into(),
        }
    }

    fn reintentar_factura(&mut self, id: i64) {
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::ReintentarFactura { id },
        }) {
            Respuesta::Ok(Cuerpo::Factura(f)) => {
                self.mensaje = if f.estado == "timbrada" {
                    format!("Timbrada con UUID {}.", f.uuid.unwrap_or_default())
                } else {
                    "Sigue pendiente: el PAC todavía no responde.".into()
                };
                self.refrescar_fiscal();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada".into(),
        }
    }

    fn ver_xml_factura(&mut self, id: i64) {
        let folio = self
            .facturas
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.folio.clone())
            .unwrap_or_else(|| id.to_string());
        match self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::XmlFactura { id },
        }) {
            Respuesta::Ok(Cuerpo::Texto(x)) => self.factura_xml = Some((folio, x)),
            Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "Respuesta inesperada".into(),
        }
    }
}

#[cfg(all(test, unix))]
mod pruebas {
    //! Pruebas de la pantalla con `egui_kittest`.
    //!
    //! Encuentra los botones **por lo que dicen** (no por coordenadas), así que
    //! si mañana se mueve algo de lugar la prueba sigue sirviendo. Y de paso
    //! renderiza la sección a un PNG para verla con los propios ojos.

    use std::sync::{Arc, Mutex};

    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use pos_api::{
        Comando, ConfigFiscalDto, Cuerpo, FacturaDto, Peticion, PosApi, Respuesta, TicketDto,
        TicketLineaDto,
    };

    use super::{PosApp, FORMAS_PAGO, PERIODICIDADES};

    /// Un doble de la API: en vez de hablar con la base, **anota lo que le
    /// pidieron**. Así la prueba verifica la intención ("la interfaz pidió
    /// facturar el día X") sin necesitar una tienda de mentira completa.
    struct ApiFalsa {
        llamados: Arc<Mutex<Vec<String>>>,
        facturas: Vec<FacturaDto>,
        /// Lo que devuelve al pedir los datos fiscales.
        fiscal: ConfigFiscalDto,
    }

    impl ApiFalsa {
        fn nueva() -> (Self, Arc<Mutex<Vec<String>>>) {
            let llamados = Arc::new(Mutex::new(Vec::new()));
            (
                ApiFalsa {
                    llamados: Arc::clone(&llamados),
                    facturas: Vec::new(),
                    fiscal: ConfigFiscalDto {
                        rfc: "XAXX010101000".into(),
                        nombre: "Abarrotes Lupita".into(),
                        regimen_fiscal: "626".into(),
                        codigo_postal: "91260".into(),
                        forma_pago: "01".into(),
                        periodicidad: "01".into(),
                        carpeta_salida: String::new(),
                        carpeta_entrada: String::new(),
                    },
                },
                llamados,
            )
        }

        fn nota(&mut self, texto: String) {
            self.llamados.lock().unwrap().push(texto);
        }
    }

    impl PosApi for ApiFalsa {
        fn ejecutar(&mut self, p: Peticion) -> Respuesta {
            match p.comando {
                Comando::ConfigFiscal => {
                    self.nota("leer configuración".into());
                    Respuesta::Ok(Cuerpo::ConfigFiscal(Box::new(self.fiscal.clone())))
                }
                Comando::Facturas { limite } => {
                    self.nota(format!("listar hasta {limite}"));
                    Respuesta::Ok(Cuerpo::Facturas(self.facturas.clone()))
                }
                Comando::GuardarConfigFiscal { config } => {
                    self.nota(format!(
                        "guardar rfc={} regimen={} cp={}",
                        config.rfc, config.regimen_fiscal, config.codigo_postal
                    ));
                    self.fiscal = config;
                    Respuesta::vacio()
                }
                Comando::FacturaGlobal {
                    fecha,
                    periodicidad,
                } => {
                    self.nota(format!("factura global {fecha} {periodicidad}"));
                    Respuesta::Ok(Cuerpo::Factura(Box::new(FacturaDto {
                        id: 1,
                        folio: format!("G{}", fecha.replace('-', "")),
                        tipo: "global".into(),
                        periodo: fecha.clone(),
                        periodicidad,
                        receptor_rfc: "XAXX010101000".into(),
                        subtotal_cents: 10_000,
                        iva_cents: 1_600,
                        total_cents: 11_600,
                        ventas: "A-1,A-2".into(),
                        estado: "pendiente".into(),
                        uuid: None,
                        ultimo_error: None,
                        intentos: 0,
                        created_at: "2026-10-11 12:00:00".into(),
                        timbrada_at: None,
                    })))
                }
                Comando::ReintentarFactura { id } => {
                    self.nota(format!("reintentar {id}"));
                    Respuesta::Ok(Cuerpo::Factura(Box::new(FacturaDto {
                        id,
                        folio: "G20261011".into(),
                        tipo: "global".into(),
                        periodo: "2026-10-11".into(),
                        periodicidad: "01".into(),
                        receptor_rfc: "XAXX010101000".into(),
                        subtotal_cents: 10_000,
                        iva_cents: 1_600,
                        total_cents: 11_600,
                        ventas: "A-1".into(),
                        estado: "timbrada".into(),
                        uuid: Some("AAAABBBB-1111-2222-3333-444455556666".into()),
                        ultimo_error: None,
                        intentos: 1,
                        created_at: "2026-10-11 12:00:00".into(),
                        timbrada_at: Some("2026-10-11 12:05:00".into()),
                    })))
                }
                Comando::NotaCredito {
                    folio,
                    fecha,
                    lineas,
                } => {
                    self.nota(format!(
                        "nota de credito {folio} {fecha} {}",
                        lineas
                            .iter()
                            .map(|l| format!("{}x{}", l.sku, l.cantidad_millis))
                            .collect::<Vec<_>>()
                            .join(",")
                    ));
                    Respuesta::Ok(Cuerpo::Factura(Box::new(FacturaDto {
                        id: 2,
                        folio: format!("NC-1-{folio}"),
                        tipo: "egreso".into(),
                        periodo: folio.clone(),
                        periodicidad: "NC".into(),
                        receptor_rfc: "XAXX010101000".into(),
                        subtotal_cents: 1_552,
                        iva_cents: 248,
                        total_cents: 1_800,
                        ventas: folio,
                        estado: "pendiente".into(),
                        uuid: None,
                        ultimo_error: None,
                        intentos: 0,
                        created_at: "2026-10-12 12:00:00".into(),
                        timbrada_at: None,
                    })))
                }
                Comando::NotaCreditoMonto {
                    folio,
                    fecha,
                    monto_cents,
                } => {
                    self.nota(format!("nota por monto {folio} {fecha} {monto_cents}"));
                    Respuesta::Ok(Cuerpo::Factura(Box::new(FacturaDto {
                        id: 4,
                        folio: format!("NC-{folio}"),
                        tipo: "egreso".into(),
                        periodo: folio.clone(),
                        periodicidad: "NC".into(),
                        receptor_rfc: "XAXX010101000".into(),
                        subtotal_cents: monto_cents * 100 / 116,
                        iva_cents: monto_cents - (monto_cents * 100 / 116),
                        total_cents: monto_cents,
                        ventas: folio,
                        estado: "pendiente".into(),
                        uuid: None,
                        ultimo_error: None,
                        intentos: 0,
                        created_at: "2026-10-12 12:00:00".into(),
                        timbrada_at: None,
                    })))
                }
                Comando::VentasPorFacturar => {
                    self.nota("ventas por facturar".into());
                    Respuesta::Ok(Cuerpo::Texto("1".into()))
                }
                Comando::FacturarVenta { folio, fecha } => {
                    self.nota(format!("factura individual {folio} {fecha}"));
                    Respuesta::Ok(Cuerpo::Factura(Box::new(FacturaDto {
                        id: 3,
                        folio: folio.clone(),
                        tipo: "individual".into(),
                        periodo: fecha.clone(),
                        periodicidad: String::new(),
                        receptor_rfc: "RES123456T1A".into(),
                        subtotal_cents: 7_759,
                        iva_cents: 1_241,
                        total_cents: 9_000,
                        ventas: folio,
                        estado: "pendiente".into(),
                        uuid: None,
                        ultimo_error: None,
                        intentos: 0,
                        created_at: "2026-10-11 12:00:00".into(),
                        timbrada_at: None,
                    })))
                }
                Comando::TicketPorFolio { folio } => {
                    self.nota(format!("ticket {folio}"));
                    Respuesta::Ok(Cuerpo::Ticket(TicketDto {
                        folio,
                        fecha: "2026-10-11 10:00:00".into(),
                        cajero: "Ana".into(),
                        cliente: String::new(),
                        estado: "completada".into(),
                        lineas: vec![
                            TicketLineaDto {
                                sku: "SKU0".into(),
                                descripcion: "Coca-Cola 600 ml".into(),
                                qty_millis: 5000,
                                precio_cents: 1800,
                                descuento_cents: 0,
                                importe_cents: 9000,
                            },
                            TicketLineaDto {
                                sku: "SKU1".into(),
                                descripcion: "Pan blanco".into(),
                                qty_millis: 1000,
                                precio_cents: 2320,
                                descuento_cents: 0,
                                importe_cents: 2320,
                            },
                        ],
                        subtotal_cents: 9758,
                        descuento_cents: 0,
                        impuesto_cents: 1562,
                        total_cents: 11320,
                        pagos: vec![],
                        cambio_cents: 0,
                    }))
                }
                Comando::SaldoPorPagar { cliente_id } => {
                    self.nota(format!("saldo cliente={cliente_id}"));
                    Respuesta::Ok(Cuerpo::Texto("150000".into()))
                }
                Comando::ComplementoPago {
                    cliente_id,
                    monto_cents,
                    fecha,
                    forma_pago,
                } => {
                    self.nota(format!(
                        "complemento cliente={cliente_id} fecha={fecha} monto={monto_cents} forma={forma_pago}"
                    ));
                    Respuesta::Ok(Cuerpo::Factura(Box::new(FacturaDto {
                        id: 5,
                        folio: "CP-5".into(),
                        tipo: "pago".into(),
                        periodo: fecha.clone(),
                        periodicidad: String::new(),
                        receptor_rfc: "RES123456T1A".into(),
                        subtotal_cents: 0,
                        iva_cents: 0,
                        total_cents: 0,
                        ventas: String::new(),
                        estado: "pendiente".into(),
                        uuid: None,
                        ultimo_error: None,
                        intentos: 0,
                        created_at: "2026-10-20 12:00:00".into(),
                        timbrada_at: None,
                    })))
                }
                Comando::XmlFactura { id } => {
                    self.nota(format!("xml {id}"));
                    Respuesta::Ok(Cuerpo::Texto("<cfdi:Comprobante Version=\"4.0\"/>".into()))
                }
                otro => {
                    self.nota(format!("otro: {}", nombre(&otro)));
                    Respuesta::vacio()
                }
            }
        }
    }

    /// Nombre corto de un comando, para que las anotaciones se lean bien.
    fn nombre(c: &Comando) -> &'static str {
        match c {
            Comando::Usuarios => "Usuarios",
            Comando::LeerConfig => "LeerConfig",
            _ => "otro",
        }
    }

    fn app_con_facturas(facturas: Vec<FacturaDto>) -> (PosApp, Arc<Mutex<Vec<String>>>) {
        let (api, llamados) = ApiFalsa::nueva();
        let mut app = PosApp::new(Box::new(ApiFalsa { facturas, ..api }), false);
        app.refrescar_fiscal();
        (app, llamados)
    }

    fn factura(estado: &str) -> FacturaDto {
        FacturaDto {
            id: 7,
            folio: "G20261011-01".into(),
            tipo: "global".into(),
            periodo: "2026-10-11".into(),
            periodicidad: "01".into(),
            receptor_rfc: "XAXX010101000".into(),
            subtotal_cents: 10_000,
            iva_cents: 1_600,
            total_cents: 11_600,
            ventas: "A-1".into(),
            estado: estado.into(),
            uuid: None,
            ultimo_error: None,
            intentos: 0,
            created_at: "2026-10-11 12:00:00".into(),
            timbrada_at: None,
        }
    }

    /// Abre la sección dentro de un arnés de egui.
    fn arnes(app: &mut PosApp) -> Harness<'_> {
        // Alto: la sección es larga y así la captura la enseña completa,
        // incluida la cobranza que va hasta abajo.
        Harness::builder()
            .with_size(egui::Vec2::new(980.0, 1700.0))
            .build_ui(|ui| app.seccion_facturacion(ui))
    }

    /// Cobrar una factura a crédito emite un complemento, no un ticket:
    /// sin ese comprobante el cliente no puede deducir lo que ya pagó.
    #[test]
    fn la_cobranza_pide_el_complemento_de_pagos() {
        let (mut app, llamados) = app_con_facturas(vec![]);
        // El cliente ya está elegido y el pago anotado en la pantalla.
        app.pago_cliente_id = 42;
        app.pago_monto = "500.00".into();
        app.pago_fecha = "2026-10-20".into();
        app.pago_forma = "03".into();

        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();
        h.get_by_label("Registrar pago").click();
        h.run();
        drop(h);

        let hechos = llamados.lock().unwrap().clone();
        assert!(
            hechos
                .iter()
                .any(|c| c == "complemento cliente=42 fecha=2026-10-20 monto=50000 forma=03"),
            "la interfaz pidió el complemento con lo que escribió el usuario: {hechos:?}"
        );
        // Se limpió el monto: no sea que se vuelva a picar el botón.
        assert!(app.pago_monto.is_empty());
    }

    /// "Cuánto debe" pregunta y muestra el saldo, antes de anotar el pago.
    #[test]
    fn cuanto_debe_pregunta_y_muestra_el_saldo() {
        let (mut app, llamados) = app_con_facturas(vec![]);
        app.pago_cliente_id = 42;

        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();
        h.get_by_label("Cuánto debe").click();
        h.run();
        drop(h);

        let hechos = llamados.lock().unwrap().clone();
        assert!(
            hechos.iter().any(|c| c == "saldo cliente=42"),
            "preguntó cuánto debe: {hechos:?}"
        );
        assert_eq!(app.pago_saldo_cents, 150_000, "muestra lo que debe");
    }

    #[test]
    fn la_seccion_arranca_plegada_y_se_abre_al_picarle() {
        let (mut app, _llamados) = app_con_facturas(vec![]);
        let mut h = arnes(&mut app);

        // Plegada: por dentro no se ve nada todavía.
        assert!(h.query_by_label("Guardar datos fiscales").is_none());

        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();

        // Abierta: ya están los datos y el botón de guardar.
        assert!(h.query_by_label("Guardar datos fiscales").is_some());
        assert!(h.query_by_label("Generar factura global").is_some());
    }

    #[test]
    fn guardar_manda_lo_que_se_escribio_en_la_pantalla() {
        let (mut app, llamados) = app_con_facturas(vec![]);
        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();

        h.get_by_label("Guardar datos fiscales").click();
        h.run();

        let hechos = llamados.lock().unwrap().clone();
        assert!(
            hechos
                .iter()
                .any(|c| c.starts_with("guardar rfc=XAXX010101000")),
            "no mandó los datos fiscales: {hechos:?}"
        );
    }

    #[test]
    fn el_boton_factura_el_dia_que_dice_la_pantalla() {
        let (mut app, llamados) = app_con_facturas(vec![]);
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
            h.get_by_label("Generar factura global").click();
            h.run();
        }
        let hechos = llamados.lock().unwrap().clone();
        // El día viene puesto por la interfaz (hoy, en hora de México).
        assert!(
            hechos
                .iter()
                .any(|c| c.starts_with("factura global 20") && c.ends_with(" 01")),
            "no generó la global del día: {hechos:?}"
        );
    }

    #[test]
    fn la_cola_muestra_el_estado_y_deja_reintentar() {
        let (mut app, llamados) = app_con_facturas(vec![factura("pendiente")]);
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
            // El renglón dice el folio, el total y el estado.
            assert!(
                h.query_by_label("G20261011-01").is_some(),
                "no mostró el folio"
            );
            assert!(h.query_by_label("$116.00").is_some(), "no mostró el total");
            assert!(
                h.query_by_label("Pendiente").is_some(),
                "no mostró el estado"
            );
            assert!(h.query_by_label("1 factura(s) esperando timbre.").is_some());

            h.get_by_label("Reintentar").click();
            h.run();
        }
        let hechos = llamados.lock().unwrap().clone();
        assert!(
            hechos.iter().any(|c| c == "reintentar 7"),
            "no reintentó la factura de la fila: {hechos:?}"
        );
    }

    #[test]
    fn una_factura_timbrada_enseña_su_uuid_y_ya_no_deja_reintentar() {
        let mut f = factura("timbrada");
        f.uuid = Some("AAAABBBB-1111-2222-3333-444455556666".into());
        let (mut app, _llamados) = app_con_facturas(vec![f]);
        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();

        assert!(
            h.query_by_label("Timbrada").is_some(),
            "no dice que está timbrada"
        );
        assert!(
            h.query_by_label("AAAABBBB-1111-2222-3333-444455556666")
                .is_some(),
            "no mostró el UUID"
        );
        // Timbrada: el botón de reintentar ya no está.
        assert!(h.query_by_label("Reintentar").is_none());
    }

    #[test]
    fn el_boton_xml_pide_el_comprobante_correcto() {
        let (mut app, llamados) = app_con_facturas(vec![factura("pendiente")]);
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
            h.get_by_label("XML").click();
            h.run();
        }
        let hechos = llamados.lock().unwrap().clone();
        assert!(
            hechos.iter().any(|c| c == "xml 7"),
            "no pidió el XML: {hechos:?}"
        );
    }

    #[test]
    fn una_factura_rechazada_muestra_el_motivo() {
        let mut f = factura("error");
        f.ultimo_error = Some("El PAC rechazó el comprobante: RFC inválido".into());
        let (mut app, _llamados) = app_con_facturas(vec![f]);
        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();
        assert!(h.query_by_label("Rechazada").is_some());
        assert!(h
            .query_by_label("El PAC rechazó el comprobante: RFC inválido")
            .is_some());
    }

    #[test]
    fn sin_facturas_la_cola_lo_dice_y_no_truena() {
        let (mut app, _llamados) = app_con_facturas(vec![]);
        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();
        assert!(h.query_by_label("Todavía no hay facturas.").is_some());
    }

    #[test]
    fn sin_datos_fiscales_avisa_en_la_cara() {
        let (api, _llamados) = ApiFalsa::nueva();
        let mut app = PosApp::new(
            Box::new(ApiFalsa {
                fiscal: ConfigFiscalDto::default(),
                facturas: vec![],
                ..api
            }),
            false,
        );
        app.refrescar_fiscal();
        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();
        assert!(
            h.query_by_label(
                "Sin los datos fiscales no se puede facturar. Llénalos una vez y listo."
            )
            .is_some(),
            "debía avisar que faltan los datos"
        );
    }

    #[test]
    fn las_listas_del_sat_estan_completas_y_no_se_repiten() {
        // Si una clave se repite dentro del mismo catálogo, el combo box queda
        // ambiguo y se guarda otra forma de pago sin que nadie lo note. Las dos
        // listas usan claves distintas del SAT, así que se revisan por separado.
        for lista in [&FORMAS_PAGO[..], &PERIODICIDADES[..]] {
            let mut vistos = std::collections::HashSet::new();
            for (clave, _) in lista {
                assert!(vistos.insert(*clave), "clave repetida: {clave}");
            }
        }
        assert_eq!(FORMAS_PAGO.len(), 5);
        assert_eq!(PERIODICIDADES.len(), 5);
        assert_eq!(super::nombre_de(&FORMAS_PAGO, "01"), "Efectivo");
        assert_eq!(super::nombre_de(&PERIODICIDADES, "05"), "Bimestral");
        // Una clave que no existe no se cae: dice "Otros".
        assert_eq!(super::nombre_de(&FORMAS_PAGO, "77"), "Otros");
    }

    /// Escribes "2" junto a las cocas y la nota va por dos, no por las cinco.
    #[test]
    fn la_nota_parcial_manda_la_cantidad_escrita_en_cada_renglon() {
        let (mut app, llamados) = app_con_facturas(vec![]);
        app.nota_folio = "A-1".into();
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
            h.get_by_label("Buscar venta").click();
            h.run();
        }
        assert_eq!(
            app.nota_renglones.len(),
            2,
            "la venta tiene dos renglones: {:?}",
            app.nota_renglones
        );
        // Devuelve dos de las cinco cocas y deja el pan en cero.
        app.nota_folio = "A-1".into();
        app.nota_fecha = "2026-10-12".into();
        app.nota_renglones[0].3 = "2".into();
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
            h.get_by_label("Hacer nota de crédito").click();
            h.run();
        }
        let hechos = llamados.lock().unwrap().clone();
        assert!(
            hechos
                .iter()
                .any(|c| c == "nota de credito A-1 2026-10-12 SKU0x2000"),
            "la nota lleva dos cocas y nada más: {hechos:?}"
        );
    }

    /// El botón de la factura individual manda el folio escrito.
    /// Devolver por dinero, no por piezas: ropa y a granel.
    #[test]
    fn la_nota_por_monto_manda_el_dinero_escrito() {
        let (mut app, llamados) = app_con_facturas(vec![]);
        app.nota_folio = "A-1".into();
        app.nota_fecha = "2026-10-12".into();
        app.nota_monto = "50.00".into();
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
            h.get_by_label("Nota por monto").click();
            h.run();
        }
        let hechos = llamados.lock().unwrap().clone();
        assert!(
            hechos
                .iter()
                .any(|c| c == "nota por monto A-1 2026-10-12 5000"),
            "$50.00 son 5000 centavos: {hechos:?}"
        );
    }

    #[test]
    fn el_boton_de_factura_individual_pide_la_factura_de_esa_venta() {
        let (mut app, llamados) = app_con_facturas(vec![]);
        app.factura_individual_folio = "A-7".into();
        app.factura_individual_fecha = "2026-10-12".into();
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
            h.get_by_label("Facturar a nombre del cliente").click();
            h.run();
        }
        let hechos = llamados.lock().unwrap().clone();
        assert!(
            hechos
                .iter()
                .any(|c| c == "factura individual A-7 2026-10-12"),
            "la factura va a nombre del cliente de esa venta: {hechos:?}"
        );
    }

    /// Y si hay ventas con RFC esperando, la pantalla lo dice: esas no entran
    /// en la global, así que quedarse callado es dejar ingresos sin reportar.
    #[test]
    fn la_pantalla_avisa_cuando_hay_ventas_con_rfc_esperando() {
        let (mut app, _) = app_con_facturas(vec![]);
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
        }
        assert_eq!(app.pendientes_rfc, 1, "el aviso viene de la API");
        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();
        assert!(
            h.query_by_label_contains("esperando factura").is_some(),
            "sin ese aviso, el ingreso se queda sin reportar"
        );
    }

    #[test]
    fn el_boton_de_la_nota_pide_la_nota_de_la_venta_escrita() {
        let (mut app, llamados) = app_con_facturas(vec![]);
        {
            let mut h = arnes(&mut app);
            h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
            h.run();
            // Sin folio no se manda nada: mejor decirlo que facturar el vacío.
            h.get_by_label("Hacer nota de crédito").click();
            h.run();
        }
        let hechos = llamados.lock().unwrap().clone();
        assert!(
            !hechos.iter().any(|c| c.starts_with("nota de credito")),
            "sin folio no se pide nota: {hechos:?}"
        );
    }

    /// Renderiza la sección a un PNG para verla con los propios ojos.
    ///
    /// No compara nada: sólo guarda la imagen. Corre con
    /// `cargo test -p pos-ui -- --nocapture` y la deja en tests/snapshots/.
    #[test]
    fn captura_de_la_seccion() {
        let (mut app, _llamados) = app_con_facturas(vec![factura("timbrada"), {
            let mut f = factura("pendiente");
            f.id = 8;
            f.folio = "G20261010-01".into();
            f.periodo = "2026-10-10".into();
            f.total_cents = 431_827;
            f
        }]);
        let mut h = arnes(&mut app);
        h.get_by_label("Facturación electrónica (CFDI 4.0)").click();
        h.run();
        h.snapshot("seccion_facturacion");
    }
}
