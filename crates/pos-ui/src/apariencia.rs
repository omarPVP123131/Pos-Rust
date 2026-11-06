//! Apariencia: tema, colores, letra, densidad y redondeo.
//!
//! Es la pantalla desde la que quien usa el programa lo deja a su gusto. Todo
//! lo de aquí es **cosa de esta computadora**: la apariencia no es un dato del
//! negocio, no vive en la base y no pasa por la API. El móvil tiene la suya.
//!
//! Cada cambio se ve al instante: se aplica sobre el mismo contexto de egui,
//! sin reiniciar nada.

use eframe::egui;

use crate::theme::{self, Apariencia, Densidad, Letra, Redondeo, Tema, ACENTOS};
use crate::PosApp;

impl PosApp {
    /// La sección de apariencia, dentro de Configuración.
    pub(crate) fn seccion_apariencia(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("Apariencia")
                .text_style(egui::TextStyle::Heading)
                .strong()
                .color(theme::acento()),
        );
        ui.label(
            egui::RichText::new(
                "Se ve al instante y se guarda en esta computadora. Cada caja puede verse \
                 distinto.",
            )
            .color(theme::texto_suave()),
        );
        ui.add_space(6.0);

        let antes = self.apariencia.clone();
        let mut cambio = false;

        // --- los temas, como tarjetas con sus colores de verdad ---
        ui.label(egui::RichText::new("Tema").strong());
        let ancho = 168.0;
        ui.horizontal_wrapped(|ui| {
            for tema in Tema::TODOS {
                let elegido = self.apariencia.tema == tema;
                if tarjeta_de_tema(ui, tema, elegido, ancho) {
                    self.apariencia.tema = tema;
                    cambio = true;
                }
            }
        });
        let elegido = self.apariencia.tema;
        ui.label(
            egui::RichText::new(elegido.descripcion())
                .weak()
                .color(theme::texto_suave()),
        );

        // --- el acento, a mano ---
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Color de acento").strong());
        ui.horizontal_wrapped(|ui| {
            if boton_de_color(
                ui,
                theme::Apariencia {
                    tema: self.apariencia.tema,
                    ..Apariencia::por_defecto()
                }
                .acento_efectivo(),
                self.apariencia.acento.is_none(),
                "El del tema",
            ) {
                self.apariencia.acento = None;
                cambio = true;
            }
            for (nombre, rgb) in ACENTOS {
                if boton_de_color(
                    ui,
                    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    self.apariencia.acento == Some(rgb),
                    nombre,
                ) {
                    self.apariencia.acento = Some(rgb);
                    cambio = true;
                }
            }
        });

        // --- letra, aire y esquinas ---
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Tamaño de letra").strong());
            for opcion in Letra::TODAS {
                if ui
                    .selectable_value(&mut self.apariencia.letra, opcion, opcion.nombre())
                    .changed()
                {
                    cambio = true;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Espacio entre las cosas").strong());
            for opcion in Densidad::TODAS {
                if ui
                    .selectable_value(&mut self.apariencia.densidad, opcion, opcion.nombre())
                    .changed()
                {
                    cambio = true;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Esquinas").strong());
            for opcion in Redondeo::TODOS {
                if ui
                    .selectable_value(&mut self.apariencia.redondeo, opcion, opcion.nombre())
                    .changed()
                {
                    cambio = true;
                }
            }
        });

        // --- los dos detalles que más se notan en pantalla chica ---
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .toggle_value(&mut self.apariencia.bordes_marcados, "Bordes marcados")
                .changed()
            {
                cambio = true;
            }
            if ui
                .toggle_value(
                    &mut self.apariencia.barra_compacta,
                    "Barra de una sola fila",
                )
                .changed()
            {
                cambio = true;
            }
            ui.label(
                egui::RichText::new(
                    "Los bordes ayudan a separar las tarjetas cuando la pantalla tiene brillo.",
                )
                .weak()
                .color(theme::texto_suave()),
            );
        });

        // --- cómo se va a ver, antes de decidir ---
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Así se ve").strong());
        vista_previa(ui, &self.apariencia);

        // --- guardar, restablecer ---
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("Guardar apariencia").color(theme::acento()),
                ))
                .clicked()
            {
                self.mensaje = match self.apariencia.guardar() {
                    Ok(()) => "Apariencia guardada.".into(),
                    Err(e) => e,
                };
            }
            if ui.button("Restablecer").clicked() {
                self.apariencia.restablecer();
                cambio = true;
                self.mensaje = "Se volvió a la apariencia de siempre.".into();
            }
        });

        if cambio {
            self.apariencia.aplicar(ui.ctx());
            // Se guarda solo: nadie quiere perder su tema porque se apagó la
            // luz. Si no se puede escribir, no pasa nada grave.
            if self.apariencia != antes {
                let _ = self.apariencia.guardar();
            }
        }
    }
}

/// Una tarjeta por tema: enseña sus colores de verdad (fondo, tarjeta y
/// acento) y se pica en cualquier parte. Devuelve `true` si la eligieron.
///
/// Es **un solo botón**, no un marco con cosas adentro: con el marco el
/// ancho dependía del contenido y el contenido del ancho, así que el rect
/// cambiaba de un frame a otro y el clic caía donde ya no estaba el control.
fn tarjeta_de_tema(ui: &mut egui::Ui, tema: Tema, elegido: bool, ancho: f32) -> bool {
    let paleta = tema.paleta();
    let respuesta = ui.add(
        egui::Button::new(
            egui::RichText::new(tema.nombre())
                .color(paleta.texto)
                .size(15.0),
        )
        .fill(paleta.panel)
        .stroke(if elegido {
            egui::Stroke::new(2.0_f32, theme::acento())
        } else {
            egui::Stroke::new(1.0_f32, paleta.borde)
        })
        .corner_radius(egui::CornerRadius::same(10))
        .min_size(egui::vec2(ancho, 48.0)),
    );

    // Las tres franjas, a la derecha: fondo, tarjeta y acento. Es lo que de
    // verdad distingue un tema de otro, más que su nombre.
    let rect = respuesta.rect;
    let pintor = ui.painter();
    let alto = 20.0;
    let mut x = rect.right() - 10.0 - 3.0 * 15.0;
    for color in [paleta.fondo, paleta.tarjeta, paleta.acento] {
        pintor.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x, rect.center().y - alto / 2.0),
                egui::vec2(12.0, alto),
            ),
            egui::CornerRadius::same(3),
            color,
        );
        x += 15.0;
    }
    respuesta.clicked()
}

/// Una pastilla de color con su nombre escrito encima, en un color que se
/// lea. Devuelve `true` si la picaron.
fn boton_de_color(ui: &mut egui::Ui, color: egui::Color32, elegido: bool, nombre: &str) -> bool {
    // La tinta se calcula: "Amarillo" sobre amarillo no se leería.
    let tinta = theme::tinta_sobre(color);
    let boton = egui::Button::new(egui::RichText::new(nombre).color(tinta).size(13.0))
        .fill(color)
        .corner_radius(egui::CornerRadius::same(14))
        .min_size(egui::vec2(24.0, 30.0));
    let respuesta = ui.add(boton);
    if elegido {
        // Se marca con un aro del acento, para que se vea cuál está puesto
        // aunque el color se parezca a otro.
        let rect = respuesta.rect.expand(3.0);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(17),
            egui::Stroke::new(2.0_f32, theme::acento()),
            egui::StrokeKind::Outside,
        );
    }
    respuesta.clicked()
}

/// Un pedazo de la pantalla de venta con las preferencias puestas, para ver
/// el resultado antes de quedarse con él.
fn vista_previa(ui: &mut egui::Ui, ap: &Apariencia) {
    let paleta = ap.paleta();
    egui::Frame::new()
        .fill(paleta.fondo)
        .corner_radius(egui::CornerRadius::same(ap.redondeo.pixeles()))
        .stroke(egui::Stroke::new(1.0_f32, paleta.borde))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("Coca-Cola 600 ml")
                        .strong()
                        .color(paleta.texto),
                );
                ui.label(
                    egui::RichText::new("2 x $18.00")
                        .color(paleta.texto_suave)
                        .size(13.0),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("$36.00")
                            .strong()
                            .size(26.0)
                            .color(paleta.acento),
                    );
                    let _ = ui.add(
                        egui::Button::new(egui::RichText::new("Cobrar").color(paleta.acento))
                            .corner_radius(ap.redondeo.pixeles())
                            .fill(paleta.tarjeta),
                    );
                });
            });
        });
}

#[cfg(all(test, unix))]
mod pruebas {
    //! Se prueba con el arnés de egui: se pica en la tarjeta del tema y se
    //! verifica que **los colores que ve la aplicación** cambiaron de verdad.
    //! Si sólo cambiara el campo, la interfaz seguiría igual y nadie se daría
    //! cuenta hasta que reiniciara.

    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use pos_api::{Peticion, PosApi, Respuesta};

    use super::*;

    /// La apariencia no le habla a nadie: no es un dato del negocio. Así que
    /// la API de la prueba no tiene que responder nada.
    pub struct ApiNula;

    impl PosApi for ApiNula {
        fn ejecutar(&mut self, _p: Peticion) -> Respuesta {
            Respuesta::vacio()
        }
    }

    /// Un archivo temporal **por prueba**: las pruebas corren en paralelo y
    /// si compartieran archivo se estarían sobrescribiendo unas a otras.
    fn ruta_temporal() -> std::path::PathBuf {
        static CONTADOR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = CONTADOR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("pos-apariencia-{}-{}", std::process::id(), n));
        let _ = std::fs::create_dir_all(&d);
        d.join("apariencia.conf")
    }

    /// Dibuja la sección **una vez** antes de devolver el arnés: hasta que no
    /// se dibuja, los controles no existen y picarlos no hace nada.
    fn arnes(app: &mut PosApp) -> Harness<'_> {
        let mut h = Harness::builder()
            .with_size(egui::Vec2::new(1100.0, 1500.0))
            .build_ui(|ui| app.seccion_apariencia(ui));
        h.run();
        h
    }

    fn app_de_prueba() -> PosApp {
        let ruta = ruta_temporal();
        let _ = std::fs::remove_file(&ruta);
        theme::fijar_ruta(Some(ruta));
        PosApp::new(Box::new(ApiNula), false)
    }

    #[test]
    fn cambiar_de_tema_cambia_los_colores_que_ve_la_interfaz() {
        let mut app = app_de_prueba();
        assert_eq!(app.apariencia.tema, Tema::POR_DEFECTO);

        let mut h = arnes(&mut app);
        // La tarjeta del tema "Café" se pica en cualquier parte de la tarjeta.
        h.get_by_label("Café").click();
        h.run();
        drop(h);

        assert_eq!(app.apariencia.tema, Tema::Cafe, "se quedó el tema nuevo");
        // Y la aplicación ya lo está usando, no hasta que reinicie.
        assert_eq!(theme::acento(), Tema::Cafe.paleta().acento);
        assert_eq!(theme::fondo(), Tema::Cafe.paleta().fondo);
    }

    #[test]
    fn el_acento_elegido_a_mano_manda_sobre_el_del_tema() {
        let mut app = app_de_prueba();
        let mut h = arnes(&mut app);
        h.get_by_label("Morado").click();
        h.run();
        drop(h);

        assert_eq!(app.apariencia.acento, Some([0x9B, 0x6E, 0xFF]));
        assert_eq!(theme::acento(), egui::Color32::from_rgb(0x9B, 0x6E, 0xFF));
        assert_ne!(theme::acento(), Tema::POR_DEFECTO.paleta().acento);
    }

    #[test]
    fn lo_que_se_elige_se_guarda_y_se_vuelve_a_leer() {
        let mut app = app_de_prueba();
        let mut h = arnes(&mut app);
        h.get_by_label("Océano").click();
        h.run();
        h.get_by_label("Grande").click();
        h.run();
        drop(h);

        assert_eq!(app.apariencia.tema, Tema::Oceano);
        assert_eq!(app.apariencia.letra, Letra::Grande);

        // En el disco quedó exactamente lo mismo que se está usando.
        let de_vuelta = Apariencia::cargar();
        assert_eq!(de_vuelta.tema, Tema::Oceano, "el tema se guardó");
        assert_eq!(de_vuelta.letra, Letra::Grande, "el tamaño se guardó");
    }

    #[test]
    fn restablecer_vuelve_a_la_apariencia_de_siempre() {
        let mut app = app_de_prueba();
        app.apariencia.tema = Tema::Cafe;
        app.apariencia.letra = Letra::Extra;
        app.apariencia.bordes_marcados = true;

        let mut h = arnes(&mut app);
        h.get_by_label("Restablecer").click();
        h.run();
        drop(h);

        assert_eq!(app.apariencia, Apariencia::por_defecto());
        assert_eq!(theme::acento(), Tema::POR_DEFECTO.paleta().acento);
    }

    #[test]
    fn un_archivo_escrito_a_mano_no_tira_el_programa() {
        let ruta = ruta_temporal();
        std::fs::write(
            &ruta,
            "tema = Inventado\nacento = basura\nletra = Gigante\n\nesto no es nada\n",
        )
        .unwrap();
        theme::fijar_ruta(Some(ruta));
        // Se queda con lo que entiende y ignora el resto: la apariencia
        // nunca puede impedir abrir la caja.
        assert_eq!(Apariencia::cargar(), Apariencia::por_defecto());
    }
}
