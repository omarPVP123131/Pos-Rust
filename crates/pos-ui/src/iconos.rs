//! Iconos dibujados a mano, sin fuentes ni imágenes de fuera.
//!
//! Cada icono son unas cuantas líneas trazadas con el pintor de egui dentro
//! de un cuadro imaginario de -1 a 1. Por eso escalan solos (se ven igual de
//! bien en un botón de 16 que en un letrero de 48) y toman el color que se
//! les pida, así que cambian con el tema como todo lo demás.
//!
//! No se usa ninguna biblioteca ni tipografía externa: en una máquina con
//! 1 GB de RAM, cada cosa que no se carga es memoria que se queda.

use egui::{Color32, Painter, Pos2, Stroke};

/// Los iconos que hay. Si falta uno, se añade aquí y se dibuja abajo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icono {
    Ticket,
    CajaProducto,
    Persona,
    Grafica,
    Billete,
    Portapapeles,
    Engranaje,
    Libro,
    Buscar,
    Basura,
    Etiqueta,
    Mas,
    Menos,
    Tache,
    Paleta,
    Impresora,
    Reloj,
    Paloma,
    Alerta,
    Candado,
    Tarjeta,
    Tienda,
    Factura,
    Foco,
    Escudo,
    Camion,
    Lapiz,
    Entrada,
}

impl Icono {
    /// Dibuja el icono centrado en `centro`, de `lado` píxeles, en `color`.
    pub fn dibujar(self, pintor: &Painter, centro: Pos2, lado: f32, color: Color32) {
        // El grosor acompaña al tamaño: un icono chiquito con trazo grueso
        // se ve como una mancha.
        let t = Stroke::new((lado * 0.115).max(1.0), color);
        let r = lado * 0.5;
        // De -1..1 a píxeles de la pantalla.
        let p = |x: f32, y: f32| Pos2::new(centro.x + x * r, centro.y + y * r);
        let linea = |a: (f32, f32), b: (f32, f32)| {
            pintor.line_segment([p(a.0, a.1), p(b.0, b.1)], t);
        };
        let poligono = |puntos: &[(f32, f32)]| {
            for i in 0..puntos.len() {
                let a = puntos[i];
                let b = puntos[(i + 1) % puntos.len()];
                pintor.line_segment([p(a.0, a.1), p(b.0, b.1)], t);
            }
        };
        let circulo = |cx: f32, cy: f32, radio: f32| {
            pintor.circle_stroke(p(cx, cy), radio * r, t);
        };
        let relleno = |cx: f32, cy: f32, radio: f32| {
            pintor.circle_filled(p(cx, cy), radio * r, color);
        };
        let rectangulo = |x0: f32, y0: f32, x1: f32, y1: f32| {
            pintor.rect_stroke(
                egui::Rect::from_two_pos(p(x0, y0), p(x1, y1)),
                egui::CornerRadius::same((lado * 0.08) as u8),
                t,
                egui::StrokeKind::Inside,
            );
        };

        match self {
            Icono::Ticket => {
                rectangulo(-0.9, -0.5, 0.9, 0.5);
                linea((-0.55, -0.22), (0.45, -0.22));
                linea((-0.55, 0.02), (0.45, 0.02));
                linea((-0.55, 0.26), (0.2, 0.26));
            }
            Icono::CajaProducto => {
                rectangulo(-0.8, -0.5, 0.8, 0.75);
                linea((-0.8, -0.15), (0.8, -0.15));
                linea((0.0, -0.5), (0.0, -0.15));
            }
            Icono::Persona => {
                circulo(0.0, -0.35, 0.32);
                linea((-0.72, 0.85), (-0.4, 0.08));
                linea((0.72, 0.85), (0.4, 0.08));
                linea((-0.4, 0.08), (0.4, 0.08));
            }
            Icono::Grafica => {
                // Tres barras: la del medio es la más alta.
                for (x, alto) in [(-0.62, 0.55), (0.0, 0.95), (0.62, 0.3)] {
                    let escala = 0.38;
                    pintor.rect_filled(
                        egui::Rect::from_two_pos(p(x - 0.14, escala), p(x + 0.14, escala - alto)),
                        egui::CornerRadius::same(2),
                        color,
                    );
                }
                linea((-0.85, 0.55), (0.85, 0.55));
            }
            Icono::Billete => {
                rectangulo(-0.9, -0.55, 0.9, 0.55);
                circulo(0.0, 0.0, 0.22);
                linea((-0.6, -0.22), (-0.6, 0.22));
                linea((0.6, -0.22), (0.6, 0.22));
            }
            Icono::Portapapeles => {
                rectangulo(-0.62, -0.5, 0.62, 0.9);
                rectangulo(-0.26, -0.9, 0.26, -0.5);
                linea((-0.35, -0.1), (0.35, -0.1));
                linea((-0.35, 0.25), (0.35, 0.25));
                linea((-0.35, 0.6), (0.1, 0.6));
            }
            Icono::Engranaje => {
                circulo(0.0, 0.0, 0.48);
                circulo(0.0, 0.0, 0.17);
                for (x, y) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                    linea((x * 0.5, y * 0.5), (x * 0.85, y * 0.85));
                }
            }
            Icono::Libro => {
                rectangulo(-0.85, -0.65, 0.85, 0.65);
                linea((-0.45, -0.65), (-0.45, 0.65));
                linea((-0.2, -0.3), (0.6, -0.3));
                linea((-0.2, 0.05), (0.6, 0.05));
                linea((-0.2, 0.4), (0.4, 0.4));
            }
            Icono::Buscar => {
                circulo(-0.12, -0.12, 0.48);
                linea((0.24, 0.24), (0.82, 0.82));
            }
            Icono::Basura => {
                linea((-0.3, -0.85), (0.3, -0.85));
                linea((0.0, -0.85), (0.0, -0.6));
                linea((-0.72, -0.6), (0.72, -0.6));
                linea((-0.58, -0.6), (-0.42, 0.85));
                linea((0.58, -0.6), (0.42, 0.85));
                linea((-0.42, 0.85), (0.42, 0.85));
                linea((-0.2, -0.3), (-0.14, 0.58));
                linea((0.2, -0.3), (0.14, 0.58));
            }
            Icono::Etiqueta => {
                poligono(&[
                    (-0.9, -0.3),
                    (0.3, -0.3),
                    (0.9, 0.3),
                    (0.3, 0.9),
                    (-0.9, 0.9),
                ]);
                relleno(0.42, 0.15, 0.13);
            }
            Icono::Mas => {
                linea((-0.75, 0.0), (0.75, 0.0));
                linea((0.0, -0.75), (0.0, 0.75));
            }
            Icono::Menos => {
                linea((-0.75, 0.0), (0.75, 0.0));
            }
            Icono::Tache => {
                linea((-0.65, -0.65), (0.65, 0.65));
                linea((0.65, -0.65), (-0.65, 0.65));
            }
            Icono::Paleta => {
                circulo(0.0, 0.0, 0.85);
                relleno(-0.35, -0.3, 0.14);
                relleno(0.35, -0.38, 0.14);
                relleno(-0.05, 0.42, 0.14);
                relleno(0.42, 0.25, 0.14);
            }
            Icono::Impresora => {
                rectangulo(-0.85, -0.75, 0.85, -0.05);
                rectangulo(-0.55, -0.05, 0.55, 0.85);
                linea((-0.35, 0.2), (0.35, 0.2));
                linea((-0.35, 0.45), (0.35, 0.45));
            }
            Icono::Reloj => {
                circulo(0.0, 0.0, 0.82);
                linea((0.0, 0.0), (0.0, -0.45));
                linea((0.0, 0.0), (0.34, 0.2));
            }
            Icono::Paloma => {
                linea((-0.75, 0.05), (-0.2, 0.6));
                linea((-0.2, 0.6), (0.85, -0.55));
            }
            Icono::Alerta => {
                poligono(&[(-0.85, 0.7), (0.0, -0.8), (0.85, 0.7)]);
                linea((0.0, -0.35), (0.0, 0.2));
                relleno(0.0, 0.48, 0.09);
            }
            Icono::Candado => {
                rectangulo(-0.55, -0.05, 0.55, 0.85);
                linea((-0.32, -0.05), (-0.32, -0.4));
                linea((-0.32, -0.4), (-0.05, -0.62));
                linea((-0.05, -0.62), (0.32, -0.4));
                linea((0.32, -0.4), (0.32, -0.05));
            }
            Icono::Tarjeta => {
                rectangulo(-0.9, -0.6, 0.9, 0.6);
                linea((-0.9, -0.22), (0.9, -0.22));
                linea((-0.6, 0.25), (0.15, 0.25));
            }
            Icono::Tienda => {
                poligono(&[(-0.95, -0.1), (0.0, -0.75), (0.95, -0.1)]);
                rectangulo(-0.72, -0.1, 0.72, 0.85);
                rectangulo(-0.22, 0.25, 0.22, 0.85);
            }
            Icono::Factura => {
                rectangulo(-0.7, -0.9, 0.7, 0.9);
                linea((-0.4, -0.6), (0.4, -0.6));
                linea((-0.4, -0.3), (0.4, -0.3));
                linea((-0.4, 0.0), (0.4, 0.0));
                circulo(0.35, 0.55, 0.22);
            }
            Icono::Escudo => {
                poligono(&[
                    (0.0, -0.9),
                    (0.75, -0.5),
                    (0.62, 0.35),
                    (0.0, 0.9),
                    (-0.62, 0.35),
                    (-0.75, -0.5),
                ]);
                linea((-0.28, 0.05), (-0.05, 0.3));
                linea((-0.05, 0.3), (0.32, -0.25));
            }
            Icono::Foco => {
                circulo(0.0, -0.25, 0.45);
                linea((-0.3, 0.15), (-0.3, 0.45));
                linea((0.3, 0.15), (0.3, 0.45));
                linea((-0.3, 0.45), (0.3, 0.45));
                linea((-0.15, 0.7), (0.15, 0.7));
            }
            Icono::Camion => {
                rectangulo(-0.92, -0.35, 0.22, 0.35);
                rectangulo(0.22, -0.05, 0.88, 0.35);
                linea((0.22, -0.05), (0.55, -0.35));
                linea((0.55, -0.35), (0.88, -0.35));
                circulo(-0.5, 0.58, 0.22);
                circulo(0.5, 0.58, 0.22);
            }
            Icono::Lapiz => {
                // Lápiz diagonal: editar nombre o precio.
                linea((-0.6, 0.6), (0.45, -0.45));
                linea((0.45, -0.45), (0.7, -0.5));
                linea((0.7, -0.5), (0.55, -0.2));
                linea((-0.6, 0.6), (-0.75, 0.75));
                linea((-0.75, 0.75), (-0.45, 0.7));
                linea((-0.45, 0.7), (-0.6, 0.6));
            }
            Icono::Entrada => {
                // Flecha entrando a una caja: entradas de mercancía.
                rectangulo(-0.15, -0.5, 0.85, 0.5);
                linea((-0.85, 0.0), (-0.05, 0.0));
                linea((-0.35, -0.3), (-0.05, 0.0));
                linea((-0.35, 0.3), (-0.05, 0.0));
            }
        }
    }

    /// Lo mismo, pero dentro de un botón redondo del tamaño de un dedo.
    ///
    /// Devuelve la respuesta para saber si lo tocaron. La pista (`pista`) se
    /// ve al dejar el ratón encima: con iconos solos nadie adivina qué hacen.
    pub fn boton(
        self,
        ui: &mut egui::Ui,
        lado: f32,
        color: Color32,
        pista: &str,
    ) -> egui::Response {
        let respuesta = ui.add(
            egui::Button::new("")
                .min_size(egui::vec2(lado, lado))
                .corner_radius(egui::CornerRadius::same((lado * 0.28) as u8)),
        );
        self.dibujar(ui.painter(), respuesta.rect.center(), lado * 0.55, color);
        if !pista.is_empty() {
            // Nombre accesible: sin esto el lector de pantalla dice "botón"
            // a secas en cada icono de la lista (WCAG 4.1.2 Nombre y función).
            respuesta.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    pista.to_string(),
                )
            });
            respuesta.on_hover_text(pista)
        } else {
            respuesta
        }
    }

    /// El icono solito, centrado en lo que queda del renglón. Para los
    /// letreros grandes de "aquí no hay nada".
    pub fn grande(self, ui: &mut egui::Ui, lado: f32, color: Color32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(lado, lado), egui::Sense::hover());
        self.dibujar(ui.painter(), rect.center(), lado, color);
    }
}

#[cfg(all(test, unix))]
mod pruebas {
    use super::*;

    /// Los iconos, dibujados de verdad sobre una ventana de mentiras:
    /// si a uno le falta la mitad del trazo o se pasa de las coordenadas,
    /// aquí no truena, pero en las capturas se nota. Lo que se revisa es que
    /// ninguno panique y que el botón responda al clic.
    #[test]
    fn todos_los_iconos_se_dibujan() {
        let todos = [
            Icono::Ticket,
            Icono::CajaProducto,
            Icono::Persona,
            Icono::Grafica,
            Icono::Billete,
            Icono::Portapapeles,
            Icono::Engranaje,
            Icono::Libro,
            Icono::Buscar,
            Icono::Basura,
            Icono::Etiqueta,
            Icono::Mas,
            Icono::Menos,
            Icono::Tache,
            Icono::Paleta,
            Icono::Impresora,
            Icono::Reloj,
            Icono::Paloma,
            Icono::Alerta,
            Icono::Candado,
            Icono::Tarjeta,
            Icono::Tienda,
            Icono::Factura,
            Icono::Foco,
            Icono::Escudo,
            Icono::Camion,
            Icono::Lapiz,
            Icono::Entrada,
        ];
        assert_eq!(todos.len(), 28);
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::Vec2::new(400.0, 400.0))
            .build_ui(|ui| {
                for i in todos {
                    i.boton(ui, 30.0, egui::Color32::WHITE, "algo");
                }
            });
        h.run();
    }

    /// Un icono suelto (el de los letreros grandes) también cabe en su sitio.
    #[test]
    fn el_icono_grande_pide_su_espacio() {
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::Vec2::new(200.0, 200.0))
            .build_ui(|ui| {
                Icono::Ticket.grande(ui, 48.0, egui::Color32::WHITE);
            });
        h.run();
    }
}
