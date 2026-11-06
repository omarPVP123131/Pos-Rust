//! Pedidos a proveedor, en la interfaz.
//!
//! Vive como pantalla propia, abierta desde el catálogo: la barra de arriba
//! ya no cabe otra pestaña, y pedir mercancía no es cada cinco minutos.
//!
//! La interfaz no mueve inventario: pide a la API y muestra.

use eframe::egui;
use pos_api::{Comando, Cuerpo, EntregaDto, NuevoProveedorDto, PedidoDto, Peticion, Respuesta};
use pos_core::{Money, Quantity};

use crate::iconos::Icono;
use crate::{theme, PosApp};

impl PosApp {
    pub(crate) fn puede_pedidos(&self) -> bool {
        self.rol() != pos_core::Role::Cajero
    }

    pub(crate) fn refrescar_pedidos(&mut self) {
        if !self.puede_pedidos() {
            return;
        }
        if let Cuerpo::Proveedores(v) = self.ejecutar(Comando::Proveedores {
            termino: String::new(),
        }) {
            self.proveedores = v;
        }
        if let Cuerpo::Pedidos(v) = self.ejecutar(Comando::Pedidos { limite: 40 }) {
            self.pedidos = v;
        }
        self.refrescar_alertas();
        if let Some(id) = self.pedido.as_ref().map(|p| p.id) {
            self.cargar_pedido(id);
        }
    }

    fn cargar_pedido(&mut self, id: i64) {
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::VerPedido { id },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => self.poner_pedido(*p),
            Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn poner_pedido(&mut self, p: PedidoDto) {
        self.pedido_entregas = p
            .lineas
            .iter()
            .map(|l| super::cantidad((l.qty_pedida_millis - l.qty_recibida_millis).max(0)))
            .collect();
        self.pedido = Some(p);
    }

    pub(crate) fn pantalla_pedidos(&mut self, ui: &mut egui::Ui) {
        theme::titulo_pantalla(
            ui,
            "Pedidos a proveedor",
            "Pides, llega la caja, recibes y el anaquel sube. Nada se inventa: lo que \
             no llegó, no se cuenta.",
            Icono::Camion,
        );

        ui.horizontal_wrapped(|ui| {
            if ui
                .add(egui::Button::new("Volver al catálogo").min_size(egui::vec2(0.0, 36.0)))
                .clicked()
            {
                self.ir_a(crate::Pantalla::Catalogo);
            }
        });
        ui.add_space(8.0);

        if !self.puede_pedidos() {
            theme::marco_tarjeta(ui, |ui| {
                ui.label("Los pedidos los hace el encargado o el dueño.");
            });
            return;
        }

        // Con un pedido abierto, la tarjeta de proveedores sólo estorba: el
        // proveedor ya aparece en el encabezado. En pantallas de 600 px se
        // comía el espacio de las líneas y del botón «Recibir».
        if self.pedido.is_none() {
            self.bloque_proveedores(ui);
            ui.add_space(8.0);
        }
        self.bloque_pedido_abierto(ui);
        ui.add_space(8.0);
        self.bloque_lista_pedidos(ui);
    }

    fn bloque_proveedores(&mut self, ui: &mut egui::Ui) {
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Proveedores", Some(Icono::Camion));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                super::campo(
                    ui,
                    "Nombre",
                    220.0,
                    &mut self.form_proveedor_nombre,
                    "Bimbo",
                );
                super::campo(
                    ui,
                    "Teléfono",
                    140.0,
                    &mut self.form_proveedor_telefono,
                    "555…",
                );
                ui.vertical(|ui| {
                    ui.label(" ");
                    if ui
                        .add(egui::Button::new("Guardar proveedor").min_size(egui::vec2(0.0, 36.0)))
                        .clicked()
                    {
                        self.guardar_proveedor();
                    }
                });
            });
            ui.add_space(6.0);
            let lista = self.proveedores.clone();
            if lista.is_empty() {
                ui.label(
                    egui::RichText::new("Todavía no hay proveedores. Escribe el nombre y guarda.")
                        .color(theme::texto_suave()),
                );
            } else {
                ui.horizontal_wrapped(|ui| {
                    for p in &lista {
                        let sel = self.pedido_proveedor == Some(p.id);
                        let texto = if p.phone.is_empty() {
                            p.name.clone()
                        } else {
                            format!("{} · {}", p.name, p.phone)
                        };
                        if ui.selectable_label(sel, texto).clicked() {
                            self.pedido_proveedor = Some(p.id);
                            self.form_proveedor_nombre = p.name.clone();
                            self.form_proveedor_telefono = p.phone.clone();
                        }
                    }
                });
            }
        });
    }

    fn bloque_pedido_abierto(&mut self, ui: &mut egui::Ui) {
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Pedido", Some(Icono::CajaProducto));
            ui.add_space(6.0);

            if self.pedido.is_none() {
                ui.horizontal_wrapped(|ui| {
                    let texto = self
                        .proveedores
                        .iter()
                        .find(|p| Some(p.id) == self.pedido_proveedor)
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "Selecciona un proveedor…".into());
                    egui::ComboBox::from_id_salt("pedido_prov")
                        .selected_text(texto)
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            let lista = self.proveedores.clone();
                            for p in lista {
                                if ui
                                    .selectable_label(self.pedido_proveedor == Some(p.id), &p.name)
                                    .clicked()
                                {
                                    self.pedido_proveedor = Some(p.id);
                                }
                            }
                        });
                    if ui
                        .add(egui::Button::new("Nuevo pedido").min_size(egui::vec2(0.0, 36.0)))
                        .clicked()
                    {
                        self.nuevo_pedido();
                    }
                });
                return;
            }

            let folio = self
                .pedido
                .as_ref()
                .map(|p| p.folio.clone())
                .unwrap_or_default();
            let estado = self
                .pedido
                .as_ref()
                .map(|p| p.estado.clone())
                .unwrap_or_default();
            let proveedor = self
                .pedido
                .as_ref()
                .map(|p| p.proveedor.clone())
                .unwrap_or_default();
            let total = self
                .pedido
                .as_ref()
                .map(|p| p.total_costo_cents)
                .unwrap_or(0);
            let abierto = estado == "abierto";
            let ya_llego = self
                .pedido
                .as_ref()
                .map(|p| p.lineas.iter().any(|l| l.qty_recibida_millis > 0))
                .unwrap_or(false);

            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!("{folio}  ·  {proveedor}"))
                        .size(18.0)
                        .strong(),
                );
                let color = match estado.as_str() {
                    "recibido" => theme::exito(),
                    "cancelado" => theme::peligro(),
                    _ => theme::acento(),
                };
                theme::pastilla(ui, &estado, color);
                ui.label(
                    egui::RichText::new(Money::from_cents(total).format_mxn())
                        .color(theme::acento())
                        .strong(),
                );
                if ui
                    .add(egui::Button::new("Ocultar detalle").min_size(egui::vec2(0.0, 30.0)))
                    .on_hover_text("Sólo cierra esta vista; el pedido no cambia.")
                    .clicked()
                {
                    self.pedido = None;
                    self.pedido_entregas.clear();
                }
            });
            ui.add_space(6.0);

            if abierto && !ya_llego {
                ui.horizontal_wrapped(|ui| {
                    super::campo(
                        ui,
                        "Producto (SKU o barras)",
                        200.0,
                        &mut self.pedido_codigo,
                        "7501",
                    );
                    super::campo(ui, "Cantidad", 80.0, &mut self.pedido_qty, "12");
                    super::campo(ui, "Costo", 90.0, &mut self.pedido_costo, "opcional");
                    ui.vertical(|ui| {
                        ui.label(" ");
                        if ui
                            .add(egui::Button::new("Agregar").min_size(egui::vec2(0.0, 36.0)))
                            .clicked()
                        {
                            self.agregar_a_pedido();
                        }
                    });
                    if !self.bajo_minimo.is_empty() {
                        ui.vertical(|ui| {
                            ui.label(" ");
                            if ui
                                .add(
                                    egui::Button::new(format!(
                                        "Pedir lo bajo mínimo ({})",
                                        self.bajo_minimo.len()
                                    ))
                                    .min_size(egui::vec2(0.0, 36.0)),
                                )
                                .on_hover_text(
                                    "Mete al pedido lo que ya está en o por debajo del mínimo.",
                                )
                                .clicked()
                            {
                                self.pedir_bajo_minimo();
                            }
                        });
                    }
                });
                ui.add_space(6.0);
            }

            let lineas = self
                .pedido
                .as_ref()
                .map(|p| p.lineas.clone())
                .unwrap_or_default();
            if lineas.is_empty() {
                let aviso = if abierto {
                    "Agrega productos: SKU o el código de barras."
                } else if estado == "cancelado" {
                    "Este pedido se canceló sin productos."
                } else {
                    "Este pedido no tiene productos."
                };
                ui.label(egui::RichText::new(aviso).color(theme::texto_suave()));
            } else {
                egui::Grid::new("pedido_lineas")
                    .num_columns(6)
                    .spacing([10.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Producto").color(theme::texto_suave()));
                        ui.label(egui::RichText::new("Pedido").color(theme::texto_suave()));
                        ui.label(egui::RichText::new("Ya llegó").color(theme::texto_suave()));
                        ui.label(egui::RichText::new("Costo").color(theme::texto_suave()));
                        ui.label(egui::RichText::new("Esta entrega").color(theme::texto_suave()));
                        ui.label("");
                        ui.end_row();
                        for (i, l) in lineas.iter().enumerate() {
                            ui.label(format!("{}  {}", l.sku, l.nombre));
                            ui.label(super::cantidad(l.qty_pedida_millis));
                            let rec = super::cantidad(l.qty_recibida_millis);
                            if l.qty_recibida_millis > 0
                                && l.qty_recibida_millis < l.qty_pedida_millis
                            {
                                // «2 de 5» se entiende de un vistazo; «2 a
                                // medias» obligaba a buscar cuánto faltaba.
                                ui.colored_label(
                                    theme::advertencia(),
                                    format!(
                                        "{rec} de {}",
                                        super::cantidad(l.qty_pedida_millis)
                                    ),
                                );
                            } else if l.qty_recibida_millis >= l.qty_pedida_millis
                                && l.qty_pedida_millis > 0
                            {
                                ui.colored_label(theme::exito(), rec);
                            } else {
                                ui.label(rec);
                            }
                            ui.label(Money::from_cents(l.costo_cents).format_mxn());
                            if abierto {
                                if i < self.pedido_entregas.len() {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.pedido_entregas[i])
                                            .desired_width(70.0),
                                    );
                                } else {
                                    ui.label("—");
                                }
                                if !ya_llego
                                    && ui
                                        .add(
                                            egui::Button::new("Quitar")
                                                .min_size(egui::vec2(64.0, 28.0)),
                                        )
                                        .clicked()
                                {
                                    self.quitar_de_pedido(l.product_id);
                                }
                            } else {
                                ui.label("—");
                                ui.label("");
                            }
                            ui.end_row();
                        }
                    });
            }

            if abierto {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if !lineas.is_empty()
                        && ui
                            .add(
                                egui::Button::new("Recibir lo que llegó")
                                    .fill(theme::acento())
                                    .min_size(egui::vec2(0.0, 40.0)),
                            )
                            .on_hover_text(
                                "Usa las cantidades de «Esta entrega». Lo que no llegó, se deja para después.",
                            )
                            .clicked()
                    {
                        self.recibir_pedido();
                    }
                    if !ya_llego
                        && ui
                            .add(egui::Button::new("Cancelar pedido").min_size(egui::vec2(0.0, 40.0)))
                            .clicked()
                    {
                        self.cancelar_pedido();
                    }
                });
            }
        });
    }

    fn bloque_lista_pedidos(&mut self, ui: &mut egui::Ui) {
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Recientes", Some(Icono::Reloj));
            ui.add_space(4.0);
            let lista = self.pedidos.clone();
            if lista.is_empty() {
                ui.label(
                    egui::RichText::new("Todavía no hay pedidos.").color(theme::texto_suave()),
                );
                return;
            }
            egui::Grid::new("pedidos_lista")
                .num_columns(5)
                .spacing([12.0, 5.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Folio").color(theme::texto_suave()));
                    ui.label(egui::RichText::new("Proveedor").color(theme::texto_suave()));
                    ui.label(egui::RichText::new("Estado").color(theme::texto_suave()));
                    ui.label(egui::RichText::new("Importe").color(theme::texto_suave()));
                    ui.label("");
                    ui.end_row();
                    for p in &lista {
                        ui.label(&p.folio);
                        ui.label(&p.proveedor);
                        ui.label(&p.estado);
                        ui.label(Money::from_cents(p.total_costo_cents).format_mxn());
                        if ui
                            .add(egui::Button::new("Abrir").min_size(egui::vec2(64.0, 28.0)))
                            .clicked()
                        {
                            self.cargar_pedido(p.id);
                        }
                        ui.end_row();
                    }
                });
        });
    }

    fn guardar_proveedor(&mut self) {
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::GuardarProveedor {
                proveedor: NuevoProveedorDto {
                    id: None,
                    name: self.form_proveedor_nombre.clone(),
                    phone: self.form_proveedor_telefono.clone(),
                    nota: String::new(),
                },
            },
        }) {
            Respuesta::Ok(Cuerpo::Texto(id)) => {
                if let Ok(n) = id.parse::<i64>() {
                    self.pedido_proveedor = Some(n);
                }
                self.mensaje = "✓ Proveedor guardado. Ya puedes pedirle.".into();
                self.form_proveedor_nombre.clear();
                self.form_proveedor_telefono.clear();
                self.refrescar_pedidos();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn nuevo_pedido(&mut self) {
        let Some(proveedor_id) = self.pedido_proveedor else {
            self.mensaje = "Selecciona un proveedor".into();
            return;
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::NuevoPedido {
                proveedor_id,
                nota: String::new(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => {
                self.mensaje = format!("✓ Pedido {} abierto. Agrega lo que necesitas.", p.folio);
                self.poner_pedido(*p);
                self.refrescar_pedidos();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn agregar_a_pedido(&mut self) {
        let Some(id) = self.pedido.as_ref().map(|p| p.id) else {
            return;
        };
        let qty = Quantity::parse(&self.pedido_qty).unwrap_or(Quantity::ZERO);
        let costo = if self.pedido_costo.trim().is_empty() {
            None
        } else {
            Money::parse(&self.pedido_costo).map(|m| m.cents())
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::AgregarAPedido {
                pedido_id: id,
                codigo: self.pedido_codigo.clone(),
                qty_millis: qty.millis(),
                costo_cents: costo,
            },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => {
                self.pedido_codigo.clear();
                self.pedido_qty.clear();
                self.pedido_costo.clear();
                self.mensaje = format!("✓ Agregado a {}. Sigue con el siguiente.", p.folio);
                self.poner_pedido(*p);
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn quitar_de_pedido(&mut self, product_id: i64) {
        let Some(id) = self.pedido.as_ref().map(|p| p.id) else {
            return;
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::QuitarDePedido {
                pedido_id: id,
                product_id,
            },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => self.poner_pedido(*p),
            Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn recibir_pedido(&mut self) {
        let Some(p) = self.pedido.clone() else {
            return;
        };
        let mut entregas = Vec::new();
        for (i, l) in p.lineas.iter().enumerate() {
            let txt = self.pedido_entregas.get(i).cloned().unwrap_or_default();
            let qty = Quantity::parse(&txt).unwrap_or(Quantity::ZERO);
            if qty.millis() > 0 {
                entregas.push(EntregaDto {
                    product_id: l.product_id,
                    qty_millis: qty.millis(),
                });
            }
        }
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::RecibirPedido { id: p.id, entregas },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => {
                self.mensaje = if p.estado == "recibido" {
                    format!("✓ Pedido {} recibido. El inventario ya subió.", p.folio)
                } else {
                    format!("✓ Se recibió parte de {}. El inventario ya subió.", p.folio)
                };
                self.poner_pedido(*p);
                self.refrescar_pedidos();
                self.refrescar_alertas();
                self.buscar("");
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn pedir_bajo_minimo(&mut self) {
        let Some(id) = self.pedido.as_ref().map(|p| p.id) else {
            self.mensaje = "Abre un pedido primero".into();
            return;
        };
        self.refrescar_alertas();
        if self.bajo_minimo.is_empty() {
            self.mensaje = "Nada está bajo el mínimo".into();
            return;
        }
        let lista = self.bajo_minimo.clone();
        let mut n = 0usize;
        for p in lista {
            let falta = (p.min_stock_millis - p.stock_millis).max(1_000);
            match self.api.ejecutar(Peticion {
                token: self.token(),
                comando: Comando::AgregarAPedido {
                    pedido_id: id,
                    codigo: p.sku,
                    qty_millis: falta,
                    costo_cents: None,
                },
            }) {
                Respuesta::Ok(Cuerpo::Pedido(ped)) => {
                    n += 1;
                    self.poner_pedido(*ped);
                }
                Respuesta::Error(e) => {
                    self.mensaje = e;
                    return;
                }
                _ => {}
            }
        }
        self.mensaje = format!("Se pidieron {n} productos que estaban bajo el mínimo");
        self.refrescar_pedidos();
    }

    fn cancelar_pedido(&mut self) {
        let Some(id) = self.pedido.as_ref().map(|p| p.id) else {
            return;
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::CancelarPedido { id },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => {
                self.mensaje = format!("Pedido {} cancelado", p.folio);
                self.poner_pedido(*p);
                self.refrescar_pedidos();
            }
            Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }
}
