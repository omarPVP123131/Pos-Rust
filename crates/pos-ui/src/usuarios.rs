//! Gestión de quienes usan el programa: alta, rol, PIN y baja.
//!
//! Vive en Configuración. La interfaz no decide permisos: pide a la API.

use eframe::egui;
use pos_api::{Comando, Cuerpo, Peticion};
use pos_core::Role;

use crate::iconos::Icono;
use crate::{theme, PosApp};

impl PosApp {
    pub(crate) fn refrescar_usuarios_admin(&mut self) {
        if !self.es_admin() {
            return;
        }
        if let Cuerpo::Usuarios(u) = self.ejecutar(Comando::Usuarios) {
            self.usuarios = u;
        }
    }

    pub(crate) fn seccion_usuarios(&mut self, ui: &mut egui::Ui) {
        if !self.es_admin() {
            return;
        }
        ui.add_space(10.0);
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Quién usa el programa", Some(Icono::Persona));
            ui.label(
                egui::RichText::new(
                    "Cajero cobra y corta caja. Supervisor descuenta, fía y ajusta. \
                     Administrador configura y da de alta a los demás.",
                )
                .size(12.0)
                .color(theme::texto_suave()),
            );
            ui.add_space(8.0);

            let lista = self.usuarios.clone();
            let yo = self.usuario.as_ref().map(|x| x.id);
            for u in &lista {
                ui.horizontal(|ui| {
                    let (av, _) =
                        ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                    theme::pintar_iniciales(ui.painter(), av, &u.display_name, theme::acento());
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&u.display_name).strong().size(15.0));
                            theme::pastilla(ui, etiqueta_rol(&u.role), color_rol(&u.role));
                        });
                        ui.label(
                            egui::RichText::new(&u.username)
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if yo != Some(u.id)
                            && ui
                                .add(
                                    theme::boton_suave("Dar de baja")
                                        .min_size(egui::vec2(0.0, 36.0)),
                                )
                                .clicked()
                        {
                            self.desactivar_usuario(u.id);
                        }
                        if ui
                            .add(theme::boton_suave("Editar").min_size(egui::vec2(0.0, 36.0)))
                            .clicked()
                        {
                            self.form_usu_id = Some(u.id);
                            self.form_usu_username = u.username.clone();
                            self.form_usu_nombre = u.display_name.clone();
                            self.form_usu_rol = Role::desde_texto(&u.role).as_str().into();
                            self.form_usu_pin.clear();
                        }
                    });
                });
                ui.add_space(6.0);
            }

            ui.separator();
            ui.add_space(4.0);
            let editando = self.form_usu_id.is_some();
            let titulo_alta = if editando {
                "Editar persona"
            } else {
                "Persona nueva"
            };
            egui::CollapsingHeader::new(egui::RichText::new(titulo_alta).strong().size(15.0))
                .id_salt("alta_usu")
                .open(if editando { Some(true) } else { None })
                .default_open(false)
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.columns(2, |cols| {
                        if !editando {
                            campo(
                                &mut cols[0],
                                "Usuario",
                                0.0,
                                &mut self.form_usu_username,
                                "ana",
                            );
                        } else {
                            cols[0].vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("Usuario")
                                        .size(12.0)
                                        .color(theme::texto_suave()),
                                );
                                ui.label(egui::RichText::new(&self.form_usu_username).strong());
                            });
                        }
                        campo(
                            &mut cols[1],
                            "Cómo se llama",
                            0.0,
                            &mut self.form_usu_nombre,
                            "Ana",
                        );
                    });
                    ui.columns(2, |cols| {
                        cols[0].vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Rol")
                                    .size(12.0)
                                    .color(theme::texto_suave()),
                            );
                            let texto = etiqueta_rol(&self.form_usu_rol);
                            egui::ComboBox::from_id_salt("rol_usu")
                                .selected_text(texto)
                                .width(ui.available_width())
                                .height(40.0)
                                .show_ui(ui, |ui| {
                                    for r in Role::ALL {
                                        if ui
                                            .selectable_label(
                                                self.form_usu_rol == r.as_str(),
                                                r.label(),
                                            )
                                            .clicked()
                                        {
                                            self.form_usu_rol = r.as_str().into();
                                        }
                                    }
                                });
                        });
                        campo(
                            &mut cols[1],
                            if editando {
                                "PIN nuevo (vacío = no cambiar)"
                            } else {
                                "PIN (4 a 8 dígitos)"
                            },
                            0.0,
                            &mut self.form_usu_pin,
                            "4321",
                        );
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        let etiqueta = if editando {
                            "Guardar persona"
                        } else {
                            "Dar de alta"
                        };
                        if ui.add(theme::boton_acento(etiqueta)).clicked() {
                            self.guardar_usuario();
                        }
                        if editando && ui.add(theme::boton_suave("Cancelar")).clicked() {
                            self.limpiar_form_usuario();
                        }
                    });
                });
        });
    }

    fn guardar_usuario(&mut self) {
        if let Some(id) = self.form_usu_id {
            let r = self.api.ejecutar(Peticion {
                token: self.token(),
                comando: Comando::ActualizarUsuario {
                    id,
                    display_name: self.form_usu_nombre.clone(),
                    role: self.form_usu_rol.clone(),
                },
            });
            if let pos_api::Respuesta::Error(e) = r {
                self.mensaje = e;
                return;
            }
            if !self.form_usu_pin.trim().is_empty() {
                let r = self.api.ejecutar(Peticion {
                    token: self.token(),
                    comando: Comando::CambiarPinUsuario {
                        id,
                        pin: self.form_usu_pin.clone(),
                    },
                });
                if let pos_api::Respuesta::Error(e) = r {
                    self.mensaje = e;
                    return;
                }
            }
            self.mensaje = "✓ Persona actualizada.".into();
        } else {
            let r = self.api.ejecutar(Peticion {
                token: self.token(),
                comando: Comando::CrearUsuario {
                    username: self.form_usu_username.clone(),
                    display_name: self.form_usu_nombre.clone(),
                    pin: self.form_usu_pin.clone(),
                    role: self.form_usu_rol.clone(),
                },
            });
            match r {
                pos_api::Respuesta::Ok(_) => {
                    self.mensaje = "✓ Persona dada de alta. Ya puede entrar con su PIN.".into()
                }
                pos_api::Respuesta::Error(e) => {
                    self.mensaje = e;
                    return;
                }
            }
        }
        self.limpiar_form_usuario();
        self.refrescar_usuarios_admin();
    }

    fn desactivar_usuario(&mut self, id: i64) {
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::DesactivarUsuario { id },
        }) {
            pos_api::Respuesta::Ok(_) => {
                self.mensaje = "Persona dada de baja".into();
                self.refrescar_usuarios_admin();
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
        }
    }

    fn limpiar_form_usuario(&mut self) {
        self.form_usu_id = None;
        self.form_usu_username.clear();
        self.form_usu_nombre.clear();
        self.form_usu_pin.clear();
        self.form_usu_rol = "cajero".into();
    }
}

fn etiqueta_rol(rol: &str) -> &'static str {
    Role::desde_texto(rol).label()
}

fn color_rol(rol: &str) -> egui::Color32 {
    match Role::desde_texto(rol) {
        Role::Admin => theme::peligro(),
        Role::Supervisor => theme::advertencia(),
        Role::Cajero => theme::exito(),
    }
}

fn campo(ui: &mut egui::Ui, etiqueta: &str, ancho: f32, valor: &mut String, pista: &str) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(etiqueta)
                .size(12.0)
                .color(theme::texto_suave()),
        );
        let ancho = if ancho <= 0.0 {
            ui.available_width()
        } else {
            ancho
        };
        ui.add_sized(
            [ancho, 40.0],
            egui::TextEdit::singleline(valor).hint_text(pista),
        );
    });
}
