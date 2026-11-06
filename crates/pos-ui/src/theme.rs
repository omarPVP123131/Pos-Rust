//! Tema y preferencias de apariencia del punto de venta.
//!
//! egui trae un tema por defecto funcional pero gris. Aquí se define el look
//! "moderno": superficies con profundidad, un acento de color, esquinas
//! redondeadas y tipografía grande (se opera con el dedo y a veces desde
//! 60 cm de distancia).
//!
//! ## Los colores no son constantes
//!
//! Cambiar de tema tiene que verse **en toda la aplicación**, así que los
//! colores se consultan con funciones (`theme::acento()`) y no como
//! constantes. En vez de pasar la paleta por parámetro por media aplicación,
//! vive en un `thread_local` que sólo se cambia desde [`aplicar`]:
//!
//! * En el programa real, `aplicar` lee las preferencias del disco.
//! * En las pruebas, nadie la cambia: cada hilo arranca con la paleta por
//!   defecto, así que las capturas no dependen del orden en que corren.
//!
//! ## Se guarda en el equipo, no en la base
//!
//! La apariencia es cosa de esta pantalla: el móvil tiene la suya. Va en un
//! archivo de texto simple (`apariencia.conf`) junto a la configuración del
//! usuario, sin dependencias nuevas: se lee y se escribe a mano.

use std::cell::RefCell;

use eframe::egui;

// ---------------------------------------------------------------- la paleta --

/// Los colores con los que se dibuja todo. Cada [`Tema`] define uno.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paleta {
    /// Color de la marca: botones principales, totales, lo que se selecciona.
    pub acento: egui::Color32,
    /// Fondo del acento, para botones suaves y selecciones.
    pub acento_tenue: egui::Color32,
    pub peligro: egui::Color32,
    /// Ámbar: "ojo, pero no es una emergencia" (respaldo pendiente, stock bajo).
    pub advertencia: egui::Color32,
    /// Verde: todo en orden.
    pub exito: egui::Color32,
    /// Detrás de todo.
    pub fondo: egui::Color32,
    /// Superficie de las barras y las ventanas.
    pub panel: egui::Color32,
    /// Tarjetas, renglones de tabla y campos de texto.
    pub tarjeta: egui::Color32,
    /// Renglones al pasar el mouse o al estar seleccionados.
    pub tarjeta_alta: egui::Color32,
    pub borde: egui::Color32,
    pub texto: egui::Color32,
    pub texto_suave: egui::Color32,
    /// `true` si las superficies son claras: cambia los visuales base de egui
    /// (el scroll, los menús y todo lo que no pintamos nosotros).
    pub claro: bool,
}

thread_local! {
    static PALETA: RefCell<Paleta> = RefCell::new(Tema::POR_DEFECTO.paleta());
}

/// Deja ver la paleta en uso sin poder prestarla: evita que alguien la
/// modifique por accidente a media pantalla.
pub fn con_paleta<T>(f: impl FnOnce(&Paleta) -> T) -> T {
    PALETA.with(|p| f(&p.borrow()))
}

pub fn acento() -> egui::Color32 {
    con_paleta(|p| p.acento)
}
pub fn acento_tenue() -> egui::Color32 {
    con_paleta(|p| p.acento_tenue)
}
pub fn peligro() -> egui::Color32 {
    con_paleta(|p| p.peligro)
}
pub fn advertencia() -> egui::Color32 {
    con_paleta(|p| p.advertencia)
}
pub fn exito() -> egui::Color32 {
    con_paleta(|p| p.exito)
}
pub fn fondo() -> egui::Color32 {
    con_paleta(|p| p.fondo)
}
pub fn panel() -> egui::Color32 {
    con_paleta(|p| p.panel)
}
pub fn tarjeta() -> egui::Color32 {
    con_paleta(|p| p.tarjeta)
}
pub fn tarjeta_alta() -> egui::Color32 {
    con_paleta(|p| p.tarjeta_alta)
}
pub fn borde() -> egui::Color32 {
    con_paleta(|p| p.borde)
}
pub fn texto() -> egui::Color32 {
    con_paleta(|p| p.texto)
}
pub fn texto_suave() -> egui::Color32 {
    con_paleta(|p| p.texto_suave)
}

/// El verde lima de la marca: cobrar, la navegación activa y la selección.
/// En los temas oscuros se usa el acento para no romper el contraste.
pub fn lima() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0xBE, 0xE3, 0x6A)
        } else {
            p.acento
        }
    })
}

pub fn lima_oscuro() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0xA6, 0xCE, 0x4E)
        } else {
            p.acento.gamma_multiply(0.82)
        }
    })
}

/// Fondo de las zonas que acompañan al lima sin gritar: badges y selección.
pub fn lima_tenue() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0xF4, 0xF9, 0xE8)
        } else {
            tenir(p.acento, 0.14)
        }
    })
}

pub fn tinta_lima() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0x22, 0x35, 0x2E)
        } else {
            tinta_sobre(p.acento)
        }
    })
}

/// La barra lateral es oscura en todos los temas: separa la operación del
/// contenido sin competir con las tarjetas.
pub fn barra_lateral() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0x22, 0x35, 0x2E)
        } else {
            p.fondo.gamma_multiply(0.68)
        }
    })
}

pub fn barra_lateral_texto() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0xDC, 0xE5, 0xDC)
        } else {
            p.texto
        }
    })
}

pub fn barra_lateral_suave() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0x93, 0xA5, 0x97)
        } else {
            p.texto_suave
        }
    })
}

pub fn barra_lateral_linea() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0x35, 0x4A, 0x41)
        } else {
            p.borde
        }
    })
}

/// Una superficie apenas distinta del blanco: totales, pies de tabla y
/// cabeceras. No es el fondo general, que va más claro.
pub fn superficie_suave() -> egui::Color32 {
    con_paleta(|p| {
        if p.claro {
            c(0xF7, 0xF7, 0xF1)
        } else {
            p.tarjeta_alta
        }
    })
}

// ------------------------------------------------------------------- temas ---

/// Los temas que trae el programa. El acento se puede cambiar aparte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tema {
    /// Verde sobre grafito. El de siempre: descansa la vista en un local
    /// con poca luz.
    Medianoche,
    /// Superficies claras. Para mostradores con mucha luz o para quien
    /// simplemente lo prefiere así.
    Dia,
    /// Negro, blanco y amarillo, con bordes marcados: para quien no ve bien
    /// los contrastes suaves.
    AltoContraste,
    /// Café cálido con acento ámbar. Se ve menos "clínico" sobre madera.
    Cafe,
    /// Azul marino con acento cian.
    Oceano,
}

impl Tema {
    /// El tema con el que abre el programa la primera vez.
    pub const POR_DEFECTO: Tema = Tema::Dia;

    pub const TODOS: [Tema; 5] = [
        Tema::Medianoche,
        Tema::Dia,
        Tema::AltoContraste,
        Tema::Cafe,
        Tema::Oceano,
    ];

    pub fn nombre(self) -> &'static str {
        match self {
            Tema::Medianoche => "Medianoche",
            Tema::Dia => "Día",
            Tema::AltoContraste => "Alto contraste",
            Tema::Cafe => "Café",
            Tema::Oceano => "Océano",
        }
    }

    /// Una línea que dice para qué es, que es lo que alguien necesita para
    /// elegir sin probarlos todos.
    pub fn descripcion(self) -> &'static str {
        match self {
            Tema::Medianoche => "Verde sobre grafito. El de siempre, descansa la vista.",
            Tema::Dia => "Superficies claras. Para mostradores con mucha luz.",
            Tema::AltoContraste => "Blanco y negro con bordes marcados. Se lee de lejos.",
            Tema::Cafe => "Cálido, con acento ámbar. Menos clínico.",
            Tema::Oceano => "Azul marino con acento cian.",
        }
    }

    pub fn paleta(self) -> Paleta {
        match self {
            Tema::Medianoche => Paleta {
                acento: c(0x3D, 0xDC, 0x97),
                acento_tenue: c(0x1E, 0x3A, 0x2E),
                peligro: c(0xFF, 0x5C, 0x5C),
                advertencia: c(0xF2, 0xC1, 0x4E),
                // Azul, no el verde del acento: si "salió bien" y la marca
                // se vieran iguales, el color ya no diría nada.
                exito: c(0x4F, 0xC3, 0xF7),
                fondo: c(0x12, 0x14, 0x1A),
                panel: c(0x1A, 0x1D, 0x26),
                tarjeta: c(0x22, 0x26, 0x31),
                tarjeta_alta: c(0x2E, 0x34, 0x42),
                borde: c(0x32, 0x38, 0x46),
                texto: c(0xE8, 0xEC, 0xF2),
                texto_suave: c(0x9A, 0xA3, 0xB2),
                claro: false,
            },
            Tema::Dia => Paleta {
                // Verde sage: la marca no compite con el lima de cobrar.
                acento: c(0x5C, 0x7A, 0x63),
                acento_tenue: c(0xE8, 0xEF, 0xE8),
                peligro: c(0xB4, 0x45, 0x3C),
                advertencia: c(0xA8, 0x76, 0x2B),
                exito: c(0x2F, 0x6F, 0x8F),
                // Off-white neutro: el papel cálido hacía que todo se viera
                // amarillento, sobre todo en pantallas baratas.
                fondo: c(0xF5, 0xF5, 0xF0),
                panel: c(0xFF, 0xFF, 0xFF),
                tarjeta: c(0xFF, 0xFF, 0xFF),
                tarjeta_alta: c(0xEF, 0xEF, 0xE8),
                borde: c(0xE3, 0xE3, 0xDA),
                texto: c(0x1E, 0x24, 0x1F),
                texto_suave: c(0x73, 0x7A, 0x72),
                claro: true,
            },
            Tema::AltoContraste => Paleta {
                acento: c(0xFF, 0xD4, 0x00),
                acento_tenue: c(0x3A, 0x33, 0x00),
                peligro: c(0xFF, 0x6B, 0x6B),
                advertencia: c(0xFF, 0xD4, 0x00),
                exito: c(0x5C, 0xFF, 0x9D),
                fondo: c(0x00, 0x00, 0x00),
                panel: c(0x00, 0x00, 0x00),
                tarjeta: c(0x14, 0x14, 0x14),
                tarjeta_alta: c(0x2E, 0x2E, 0x2E),
                borde: c(0xFF, 0xFF, 0xFF),
                texto: c(0xFF, 0xFF, 0xFF),
                texto_suave: c(0xD6, 0xD6, 0xD6),
                claro: false,
            },
            Tema::Cafe => Paleta {
                acento: c(0xE8, 0xA0, 0x54),
                acento_tenue: c(0x3A, 0x2A, 0x1B),
                // Rojo de verdad: con el ámbar de la marca al lado, un rojo
                // salmón se confunde y el peligro deja de verse como peligro.
                peligro: c(0xE8, 0x45, 0x3C),
                advertencia: c(0xFF, 0x8C, 0x1A),
                exito: c(0x7E, 0xC4, 0x6B),
                fondo: c(0x16, 0x11, 0x0E),
                panel: c(0x22, 0x1A, 0x15),
                tarjeta: c(0x2E, 0x24, 0x1D),
                tarjeta_alta: c(0x3D, 0x30, 0x27),
                borde: c(0x45, 0x36, 0x2B),
                texto: c(0xF5, 0xEC, 0xE4),
                texto_suave: c(0xB4, 0xA3, 0x95),
                claro: false,
            },
            Tema::Oceano => Paleta {
                acento: c(0x38, 0xCF, 0xE0),
                acento_tenue: c(0x11, 0x33, 0x3D),
                peligro: c(0xFF, 0x6E, 0x7A),
                advertencia: c(0xFF, 0xC2, 0x4B),
                exito: c(0x4A, 0xD9, 0x91),
                fondo: c(0x0B, 0x14, 0x1F),
                panel: c(0x12, 0x1E, 0x2D),
                tarjeta: c(0x1A, 0x2A, 0x3D),
                tarjeta_alta: c(0x24, 0x3A, 0x52),
                borde: c(0x2C, 0x45, 0x60),
                texto: c(0xE4, 0xF1, 0xFA),
                texto_suave: c(0x8F, 0xA8, 0xBD),
                claro: false,
            },
        }
    }
}

const fn c(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

// ------------------------------------------------------------ preferencias ---

/// Qué tan grande se ve todo. Se multiplica por el tamaño base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Letra {
    Chica,
    Normal,
    Grande,
    Extra,
}

impl Letra {
    pub const TODAS: [Letra; 4] = [Letra::Chica, Letra::Normal, Letra::Grande, Letra::Extra];

    pub fn nombre(self) -> &'static str {
        match self {
            Letra::Chica => "Chica",
            Letra::Normal => "Normal",
            Letra::Grande => "Grande",
            Letra::Extra => "Extra grande",
        }
    }

    pub fn factor(self) -> f32 {
        match self {
            Letra::Chica => 0.9,
            Letra::Normal => 1.0,
            Letra::Grande => 1.15,
            Letra::Extra => 1.3,
        }
    }
}

/// Cuánto aire hay entre las cosas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Densidad {
    /// Cabe más en la pantalla: para pantallas chicas o catálogos grandes.
    Compacta,
    Normal,
    /// Botones más grandes, más separación: se acierta más con el dedo.
    Comoda,
}

impl Densidad {
    pub const TODAS: [Densidad; 3] = [Densidad::Compacta, Densidad::Normal, Densidad::Comoda];

    pub fn nombre(self) -> &'static str {
        match self {
            Densidad::Compacta => "Compacta",
            Densidad::Normal => "Normal",
            Densidad::Comoda => "Cómoda",
        }
    }

    /// (separación entre elementos, relleno de los botones)
    pub fn medidas(self) -> (egui::Vec2, egui::Vec2) {
        match self {
            Densidad::Compacta => (egui::vec2(6.0, 4.0), egui::vec2(8.0, 6.0)),
            Densidad::Normal => (egui::vec2(8.0, 8.0), egui::vec2(12.0, 10.0)),
            Densidad::Comoda => (egui::vec2(12.0, 12.0), egui::vec2(18.0, 14.0)),
        }
    }

    /// Alto mínimo de los controles (botones, casillas, listas).
    ///
    /// "Normal" es el alto de siempre: subirlo cambia **toda** la interfaz de
    /// golpe, así que es algo que se elige, no algo que se impone.
    pub fn alto_de_control(self) -> f32 {
        match self {
            Densidad::Compacta => 16.0,
            Densidad::Normal => 18.0,
            Densidad::Comoda => 28.0,
        }
    }

    /// Alto de los renglones de las tablas.
    pub fn alto_renglon(self) -> f32 {
        match self {
            Densidad::Compacta => 22.0,
            Densidad::Normal => 28.0,
            Densidad::Comoda => 36.0,
        }
    }
}

/// Cuánto se redondean las esquinas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redondeo {
    Cuadrado,
    Suave,
    Redondo,
    MuyRedondo,
}

impl Redondeo {
    pub const TODOS: [Redondeo; 4] = [
        Redondeo::Cuadrado,
        Redondeo::Suave,
        Redondeo::Redondo,
        Redondeo::MuyRedondo,
    ];

    pub fn nombre(self) -> &'static str {
        match self {
            Redondeo::Cuadrado => "Cuadrado",
            Redondeo::Suave => "Suave",
            Redondeo::Redondo => "Redondo",
            Redondeo::MuyRedondo => "Muy redondo",
        }
    }

    pub fn pixeles(self) -> u8 {
        match self {
            Redondeo::Cuadrado => 2,
            Redondeo::Suave => 6,
            Redondeo::Redondo => 10,
            Redondeo::MuyRedondo => 16,
        }
    }
}

/// Doce colores para el acento, más "el del tema". Elegir de una lista es más
/// rápido que mover un selector y siempre da algo que se lee bien.
pub const ACENTOS: [(&str, [u8; 3]); 12] = [
    ("Verde menta", [0x3D, 0xDC, 0x97]),
    ("Esmeralda", [0x1E, 0xA1, 0x6B]),
    ("Cian", [0x38, 0xCF, 0xE0]),
    ("Azul", [0x4E, 0x8C, 0xFF]),
    ("Morado", [0x9B, 0x6E, 0xFF]),
    ("Rosa", [0xFF, 0x6E, 0xA8]),
    ("Rojo", [0xFF, 0x5C, 0x5C]),
    ("Naranja", [0xFF, 0x8A, 0x3D]),
    ("Ámbar", [0xF2, 0xC1, 0x4E]),
    ("Amarillo", [0xFF, 0xD4, 0x00]),
    ("Lima", [0xA8, 0xE0, 0x3D]),
    ("Turquesa", [0x2D, 0xD4, 0xBF]),
];

/// Todo lo que se puede personalizar. Se guarda en el disco tal cual.
#[derive(Clone, Debug, PartialEq)]
pub struct Apariencia {
    pub tema: Tema,
    /// `None` = el acento que trae el tema. `Some([r, g, b])` = uno a medida.
    pub acento: Option<[u8; 3]>,
    pub letra: Letra,
    pub densidad: Densidad,
    pub redondeo: Redondeo,
    /// Separa las tarjetas del fondo con un borde, en vez de sólo relleno.
    pub bordes_marcados: bool,
    /// Pone el nombre de la pantalla y la hora en la barra de arriba.
    /// Una sola fila en lugar de dos: para pantallas chicas.
    pub barra_compacta: bool,
}

impl Default for Apariencia {
    fn default() -> Self {
        Apariencia {
            tema: Tema::Dia,
            acento: None,
            letra: Letra::Normal,
            densidad: Densidad::Normal,
            redondeo: Redondeo::Redondo,
            bordes_marcados: true,
            barra_compacta: false,
        }
    }
}

impl Apariencia {
    pub fn por_defecto() -> Self {
        Self::default()
    }

    /// El acento que se va a usar: el elegido a mano, o el del tema.
    pub fn acento_efectivo(&self) -> egui::Color32 {
        match self.acento {
            Some([r, g, b]) => c(r, g, b),
            None => self.tema.paleta().acento,
        }
    }

    /// La paleta ya con el acento elegido por el usuario.
    pub fn paleta(&self) -> Paleta {
        let mut p = self.tema.paleta();
        let acento = self.acento_efectivo();
        p.acento = acento;
        // El tenue es el mismo acento, muy oscurecido: así sigue combinando
        // aunque alguien elija un color que el tema no conocía.
        p.acento_tenue = if p.claro {
            acento.gamma_multiply(0.18)
        } else {
            acento.gamma_multiply(0.22)
        };
        if self.bordes_marcados {
            p.borde = if p.claro {
                c(0xD8, 0xDA, 0xD2)
            } else {
                c(0x51, 0x5C, 0x70)
            };
        }
        p
    }

    /// Pone el tema en el contexto de egui. Llamar cada vez que algo cambie.
    pub fn aplicar(&self, ctx: &egui::Context) {
        aplicar_con(ctx, self);
    }
}

// ------------------------------------------------------------------ estilo ---

/// Aplica unas preferencias: colores, tipografía, densidad y redondeo.
pub fn aplicar_con(ctx: &egui::Context, ap: &Apariencia) {
    PALETA.with(|p| *p.borrow_mut() = ap.paleta());
    let paleta = ap.paleta();

    let mut estilo = (*ctx.style()).clone();

    let (item_spacing, button_padding) = ap.densidad.medidas();
    estilo.spacing.item_spacing = item_spacing;
    estilo.spacing.button_padding = button_padding;
    estilo.spacing.window_margin = egui::Margin::same(16);
    estilo.spacing.menu_margin = egui::Margin::same(10);
    estilo.spacing.interact_size.y = ap.densidad.alto_de_control();

    let mut visuales = if paleta.claro {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuales.panel_fill = paleta.panel;
    visuales.window_fill = paleta.tarjeta;
    visuales.extreme_bg_color = paleta.fondo;
    visuales.faint_bg_color = paleta.tarjeta;
    visuales.override_text_color = Some(paleta.texto);
    visuales.window_stroke = egui::Stroke::new(1.0_f32, paleta.borde);
    visuales.window_corner_radius = egui::CornerRadius::same(16);
    visuales.window_shadow = sombra();
    visuales.window_highlight_topmost = false;
    // Los campos de texto se distinguen del fondo de la tarjeta: si fueran
    // del mismo blanco, nadie ve dónde escribir.
    visuales.widgets.inactive.weak_bg_fill = paleta.fondo;
    visuales.widgets.hovered.weak_bg_fill = paleta.tarjeta_alta;
    visuales.widgets.noninteractive.weak_bg_fill = paleta.panel;

    let radio = egui::CornerRadius::same(ap.redondeo.pixeles());
    // Los bordes se dibujan siempre que el tema los pida: en alto contraste
    // son lo que separa una tarjeta de otra, no adorno.
    let trazo = if ap.bordes_marcados || matches!(ap.tema, Tema::AltoContraste) {
        egui::Stroke::new(1.0_f32, paleta.borde)
    } else {
        egui::Stroke::NONE
    };

    for estado in [
        &mut visuales.widgets.noninteractive,
        &mut visuales.widgets.inactive,
        &mut visuales.widgets.hovered,
        &mut visuales.widgets.active,
        &mut visuales.widgets.open,
    ] {
        estado.corner_radius = radio;
        estado.expansion = 0.0;
        estado.bg_stroke = trazo;
    }
    visuales.widgets.noninteractive.bg_fill = paleta.tarjeta;
    // Los controles (combos, botones secundarios, campos) van en el color
    // del fondo: encima de una tarjeta blanca se ven, no se tragan.
    visuales.widgets.inactive.bg_fill = paleta.fondo;
    visuales.widgets.hovered.bg_fill = paleta.tarjeta_alta;
    visuales.widgets.active.bg_fill = paleta.acento_tenue;
    visuales.widgets.open.bg_fill = paleta.tarjeta;
    visuales.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, paleta.acento);
    visuales.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, paleta.borde);
    visuales.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, paleta.borde);
    visuales.selection.bg_fill = paleta.acento_tenue;
    visuales.selection.stroke = egui::Stroke::new(1.0_f32, paleta.acento);

    // Barra de scroll: finita pero SIEMPRE a la vista, y pintada con el color
    // de la letra (no con el del relleno) para que se vea igual en el tema
    // claro y en el oscuro. Si no se ve, nadie se entera de que abajo hay
    // más y cosas como los reportes quedan fuera del alcance.
    estilo.spacing.scroll = egui::style::ScrollStyle {
        floating: true,
        bar_width: 12.0_f32,
        floating_width: 5.0_f32,
        floating_allocated_width: 8.0_f32,
        foreground_color: true,
        dormant_background_opacity: 0.0_f32,
        dormant_handle_opacity: 0.85_f32,
        active_background_opacity: 0.4_f32,
        active_handle_opacity: 1.0_f32,
        interact_background_opacity: 0.6_f32,
        interact_handle_opacity: 1.0_f32,
        ..egui::style::ScrollStyle::thin()
    };

    estilo.visuals = visuales;

    // Tipografía: se lee desde lejos y se toca con el dedo. El factor de
    // letra escala todo junto, para que nada se quede chico.
    use egui::{FontId, TextStyle};
    let f = ap.letra.factor();
    let p = |tamaño: f32| FontId::proportional((tamaño * f).round());
    estilo.text_styles.insert(TextStyle::Small, p(14.0));
    // El monoespaciado se queda en el tamaño de siempre (12): va dentro de
    // tablas, y agrandarlo desacomodaría los renglones.

    estilo.text_styles.insert(TextStyle::Body, p(18.0));
    estilo.text_styles.insert(TextStyle::Button, p(19.0));
    estilo.text_styles.insert(TextStyle::Heading, p(28.0));
    estilo
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(12.0 * f));
    estilo
        .text_styles
        .insert(TextStyle::Name("Total".into()), p(50.0));
    estilo
        .text_styles
        .insert(TextStyle::Name("Precio".into()), p(26.0));
    estilo
        .text_styles
        .insert(TextStyle::Name("Titulo".into()), p(36.0));

    ctx.set_style(estilo);
}

/// Como se veía antes: el tema de siempre con las medidas de siempre.
pub fn aplicar(ctx: &egui::Context) {
    aplicar_con(ctx, &Apariencia::por_defecto());
}

/// El color de texto que se lee sobre un relleno dado.
///
/// Un botón relleno de verde menta necesita letra negra; el mismo botón en
/// morado necesita letra blanca. Se calcula con la luminancia, que es lo que
/// usa WCAG para decidir contraste.
pub fn tinta_sobre(fondo: egui::Color32) -> egui::Color32 {
    let luminancia = 0.299 * fondo.r() as f32 + 0.587 * fondo.g() as f32 + 0.114 * fondo.b() as f32;
    if luminancia > 150.0 {
        egui::Color32::from_rgb(0x18, 0x18, 0x18)
    } else {
        egui::Color32::WHITE
    }
}

/// Etiqueta de "total grande" usada en el panel del ticket.
pub fn monto_grande(monto: pos_core::Money) -> egui::RichText {
    egui::RichText::new(monto.format_mxn())
        .text_style(egui::TextStyle::Name("Total".into()))
        .strong()
        .color(acento())
}

/// Ancho usable de este `Ui`. En Windows el primer frame de un panel
/// a veces reporta `inf` o un número enorme: si se usa para `columns` o
/// `set_min_width`, el proceso se aborta.
pub fn ancho_util(ui: &egui::Ui) -> f32 {
    acotar_ancho(ui.available_width())
}

/// El mismo recorte, sin un `Ui`: para pruebas y para `cuantas_columnas`.
pub fn acotar_ancho(w: f32) -> f32 {
    if w.is_finite() && w > 1.0 && w < 4_000.0 {
        w
    } else {
        640.0
    }
}

/// Cuántas columnas caben, nunca 0 y nunca un millón.
pub fn cuantas_columnas(ui: &egui::Ui, min_ancho: f32) -> usize {
    cuantas_columnas_de(ui.available_width(), min_ancho)
}

/// Igual que [`cuantas_columnas`], con un ancho suelto (el de un panel).
pub fn cuantas_columnas_de(ancho: f32, min_ancho: f32) -> usize {
    let n = (acotar_ancho(ancho) / min_ancho.max(1.0)).floor();
    if !n.is_finite() {
        return 1;
    }
    (n as u32).clamp(1, 8) as usize
}

/// Una tarjeta: el relleno y el borde del tema, sin inventar colores sueltos
/// por la aplicación.
/// Una tarjeta: el marco donde vive cada cosa.
///
/// Lleva una sombra suave y su borde, que es lo que hace que se vea
/// *encima* del fondo y no pegada a él. Sin esa sombra la interfaz se siente
/// plana: todo es del mismo color y nada se separa de nada.
pub fn marco_tarjeta(ui: &mut egui::Ui, contenido: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(tarjeta())
        .corner_radius(egui::CornerRadius::same(14))
        .stroke(egui::Stroke::new(1.2_f32, borde()))
        .inner_margin(egui::Margin::same(14))
        .shadow(sombra())
        .show(ui, |ui| {
            // Sin esto la tarjeta se encoge al texto y quedan islas con
            // huecos a los lados: parece un borrador, no un programa.
            // En un renglón horizontal el ancho viene infinito: no se pide
            // eso, porque egui termina reservando la pantalla entera (y
            // en Windows un `set_min_width(inf)` llegó a SIGSEGV).
            let w = ui.available_width();
            if w.is_finite() && w > 1.0 && w < 4_000.0 {
                ui.set_min_width(w);
            }
            contenido(ui);
        });
}

/// La sombra de las tarjetas: corta y hacia abajo, como una hoja apoyada.
/// En el tema oscuro se apaga, porque ahí la luz no funciona igual y la
/// sombra negra sólo ensucia.
pub fn sombra() -> egui::Shadow {
    if PALETA.with(|p| p.borrow().claro) {
        egui::Shadow {
            offset: [0, 5],
            blur: 18,
            spread: 0,
            color: egui::Color32::from_black_alpha(52),
        }
    } else {
        egui::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: egui::Color32::from_black_alpha(140),
        }
    }
}

/// Mezcla un color con el fondo del panel.
///
/// Da los rellenos suaves de las pastillas y los avatares: se ve de color,
/// pero se puede leer encima. `cuanto` es cuánto del color se le pone
/// (0.16 suele ser lo justo).
pub fn tenir(color: egui::Color32, cuanto: f32) -> egui::Color32 {
    let f = panel();
    let c = cuanto.clamp(0.0, 1.0);
    let mezcla = |a: u8, b: u8| (a as f32 * (1.0 - c) + b as f32 * c).round() as u8;
    egui::Color32::from_rgb(
        mezcla(f.r(), color.r()),
        mezcla(f.g(), color.g()),
        mezcla(f.b(), color.b()),
    )
}

/// Una pastilla: letra chiquita de color sobre un fondo del mismo color,
/// muy rebajado. Sirve para los estados ("bajo mínimo", "completada") y se
/// lee de un golpe entre un montón de renglones iguales.
pub fn pastilla(ui: &mut egui::Ui, texto: &str, color: egui::Color32) {
    let galley =
        ui.fonts(|f| f.layout_no_wrap(texto.to_string(), egui::FontId::proportional(12.0), color));
    let tam = galley.size() + egui::vec2(16.0, 8.0);
    let (rect, _) = ui.allocate_exact_size(tam, egui::Sense::hover());
    let radio = egui::CornerRadius::same((rect.height() * 0.5) as u8);
    ui.painter().rect_filled(rect, radio, tenir(color, 0.16));
    ui.painter()
        .galley(rect.min + egui::vec2(8.0, 4.0), galley, color);
}

/// El título de una pantalla: un cuadro con el icono, el título bien grande
/// y una línea que dice para qué sirve.
///
/// Es lo primero que se ve al cambiar de pestaña y lo que da el tono: sin
/// esto, todas las pantallas empiezan igual (una palabra y una raya) y ninguna
/// se reconoce de lejos.
pub fn titulo_pantalla(
    ui: &mut egui::Ui,
    titulo: &str,
    subtitulo: &str,
    icono: crate::iconos::Icono,
) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(r, egui::CornerRadius::same(14), tenir(acento(), 0.14));
        icono.dibujar(ui.painter(), r.center(), 28.0, acento());
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(titulo)
                    .size(26.0)
                    .strong()
                    .color(texto()),
            );
            ui.label(
                egui::RichText::new(subtitulo)
                    .size(13.0)
                    .color(texto_suave()),
            );
        });
    });
    ui.add_space(10.0);
}

/// El título de una sección: una rayita del color de acento, el título y,
/// si se quiere, un icono. La rayita es lo que dice "aquí empieza algo".
pub fn seccion(ui: &mut egui::Ui, titulo: &str, icono: Option<crate::iconos::Icono>) {
    ui.horizontal(|ui| {
        let alto = 16.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, alto), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(2), acento());
        ui.add_space(5.0);
        if let Some(i) = icono {
            let (r, _) = ui.allocate_exact_size(egui::vec2(alto, alto), egui::Sense::hover());
            i.dibujar(ui.painter(), r.center(), 15.0, acento());
            ui.add_space(4.0);
        }
        ui.label(
            egui::RichText::new(titulo)
                .strong()
                .size(14.0)
                .color(texto()),
        );
    });
}

/// Botón principal: relleno del acento, 40 px. Guardar, dar de alta, imprimir.
pub fn boton_acento(etiqueta: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(etiqueta.into()).color(tinta_sobre(acento())))
        .fill(acento())
        .min_size(egui::vec2(140.0, 40.0))
        .corner_radius(egui::CornerRadius::same(10))
}

/// Botón secundario: borde, sin relleno de marca. Detectar, cancelar, actualizar.
pub fn boton_suave(etiqueta: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(etiqueta.into()).color(texto()))
        .fill(fondo())
        .stroke(egui::Stroke::new(1.0_f32, borde()))
        .min_size(egui::vec2(120.0, 40.0))
        .corner_radius(egui::CornerRadius::same(10))
}

/// Botón de peligro: fondo rojo tenue con letra roja. Mermas, dar de baja.
/// Se distingue del acento (guardar) a un golpe de vista.
pub fn boton_peligro(etiqueta: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(etiqueta.into())
            .color(peligro())
            .strong(),
    )
    .fill(tenir(peligro(), 0.14))
    .stroke(egui::Stroke::new(1.2_f32, peligro()))
    .min_size(egui::vec2(120.0, 40.0))
    .corner_radius(egui::CornerRadius::same(10))
}

/// Pinta un degradado suave dentro de un rectángulo, de un color a otro.
///
/// egui no trae degradados, así que se hace a tiras: con catorce el ojo ya no
/// ve el escalón. La primera y la última tira conservan las esquinas del
/// rectángulo para que un cuadro redondeado no acabe en pico.
///
/// Es lo que quita lo plano de los encabezados: una franja de un solo color se
/// ve de cartón; la misma franja con el color yéndose hacia abajo se ve de
/// aplicación de verdad.
pub fn degradado(
    pintor: &egui::Painter,
    rect: egui::Rect,
    de: egui::Color32,
    hasta: egui::Color32,
    esquinas: egui::CornerRadius,
) {
    const TIRAS: usize = 14;
    let alto = rect.height() / TIRAS as f32;
    for i in 0..TIRAS {
        let t = i as f32 / (TIRAS - 1) as f32;
        let paso = if i == 0 {
            egui::CornerRadius {
                sw: 0,
                se: 0,
                ..esquinas
            }
        } else if i == TIRAS - 1 {
            egui::CornerRadius {
                nw: 0,
                ne: 0,
                ..esquinas
            }
        } else {
            egui::CornerRadius::ZERO
        };
        pintor.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.min.y + i as f32 * alto),
                egui::vec2(rect.width(), alto + 0.6),
            ),
            paso,
            tenir_entre(de, hasta, t),
        );
    }
}

/// Mezcla dos colores: `0.0` da el primero, `1.0` da el segundo.
pub fn tenir_entre(de: egui::Color32, hasta: egui::Color32, cuanto: f32) -> egui::Color32 {
    let c = cuanto.clamp(0.0, 1.0);
    let m = |a: u8, b: u8| (a as f32 * (1.0 - c) + b as f32 * c).round() as u8;
    egui::Color32::from_rgb(
        m(de.r(), hasta.r()),
        m(de.g(), hasta.g()),
        m(de.b(), hasta.b()),
    )
}

/// Iniciales que se distinguen: se saltan "de/la/ml" y, si hay un número
/// ("Producto 0", "Coca 600"), lo usan. Así doce baldosas no dicen todas "PN".
pub fn iniciales_de(texto: &str) -> String {
    const VACIAS: &[&str] = &[
        "de", "del", "la", "el", "los", "las", "y", "o", "un", "una", "uno", "the", "of", "en",
        "numero", "número", "ml", "kg", "g", "l", "lt", "pza", "pz", "pzas",
    ];
    let partes: Vec<&str> = texto
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .filter(|p| !VACIAS.contains(&p.to_lowercase().as_str()))
        .collect();
    if partes.is_empty() {
        return "?".into();
    }
    let primera = partes[0].chars().next().unwrap().to_uppercase().to_string();
    if let Some(d) = partes
        .iter()
        .find(|p| p.chars().all(|c| c.is_ascii_digit()))
    {
        // Una cifra: P0, P1. Dos cifras: 10, 11 (si no, 10 se veía como P1).
        // Tres o más (600 ml): letra + primera cifra → C6.
        let n = d.chars().count();
        if n == 2 {
            return d.chars().take(2).collect();
        }
        let dig = d.chars().next().unwrap();
        return format!("{primera}{dig}");
    }
    if partes.len() >= 2 {
        let segunda = partes[1]
            .chars()
            .next()
            .unwrap()
            .to_uppercase()
            .next()
            .unwrap();
        format!("{primera}{segunda}")
    } else {
        let mut it = partes[0].chars();
        let a = it.next().unwrap().to_uppercase().to_string();
        match it.next() {
            Some(b) => format!("{a}{}", b.to_uppercase()),
            None => a,
        }
    }
}

/// Un cuadrito con las iniciales: la primera letra (o las dos primeras
/// palabras) en un fondo del color que se pida.
///
/// Da color y orden: en una lista de clientes o en el catálogo, el ojo
/// encuentra lo que busca por la forma antes de leer.
pub fn pintar_iniciales(
    pintor: &egui::Painter,
    rect: egui::Rect,
    texto: &str,
    color: egui::Color32,
) {
    let radio = egui::CornerRadius::same((rect.width() * 0.28) as u8);
    pintor.rect_filled(rect, radio, tenir(color, 0.20));
    let iniciales = iniciales_de(texto);
    pintor.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        iniciales,
        egui::FontId::proportional((rect.width() * 0.42).max(9.0)),
        color,
    );
}

/// Colores de anaquel (bosque, café, terracota, salvia…). No es el arcoíris:
/// es lo que hace que dos productos vecinos no se vean clones.
pub fn color_de_etiqueta(nombre: &str) -> egui::Color32 {
    const TINTES: [[u8; 3]; 8] = [
        [0x2F, 0x5D, 0x50],
        [0x8B, 0x5E, 0x3C],
        [0xC4, 0x6A, 0x3A],
        [0x5B, 0x7F, 0x5A],
        [0xA3, 0x56, 0x4A],
        [0x3D, 0x6B, 0x7A],
        [0xB0, 0x86, 0x4A],
        [0x6B, 0x4F, 0x6B],
    ];
    let h = nombre
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let [r, g, b] = TINTES[h as usize % TINTES.len()];
    c(r, g, b)
}

/// El hueco de "foto" de cada producto: un empaque de anaquel, no un
/// recuadro plano. Sin archivos ni RAM extra: se pinta.
pub fn pintar_foto_producto(pintor: &egui::Painter, rect: egui::Rect, nombre: &str) {
    let color = color_de_etiqueta(nombre);
    let radio = egui::CornerRadius::same((rect.width() * 0.22).clamp(6.0, 14.0) as u8);
    pintor.rect_filled(rect, radio, color);
    let luz = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(rect.right(), rect.top() + rect.height() * 0.45),
    );
    pintor.rect_filled(
        luz,
        egui::CornerRadius {
            nw: radio.nw,
            ne: radio.ne,
            sw: 0,
            se: 0,
        },
        tenir_entre(color, egui::Color32::WHITE, 0.28),
    );
    pintor.hline(
        (rect.left() + 6.0)..=(rect.right() - 6.0),
        rect.top() + 3.0,
        egui::Stroke::new(1.4_f32, egui::Color32::from_white_alpha(70)),
    );
    // Franja de etiqueta abajo: es lo que hace que se lea como empaque
    // y no como un recuadro de iniciales.
    let franja = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - rect.height() * 0.28),
        rect.right_bottom(),
    );
    pintor.rect_filled(
        franja,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: radio.sw,
            se: radio.se,
        },
        tenir_entre(color, egui::Color32::BLACK, 0.22),
    );
    let iniciales = iniciales_de(nombre);
    pintor.text(
        rect.center() + egui::vec2(0.0, -rect.height() * 0.06),
        egui::Align2::CENTER_CENTER,
        iniciales,
        egui::FontId::proportional((rect.width() * 0.34).max(11.0)),
        tinta_sobre(color),
    );
}

// ------------------------------------------------------------- persistencia --

#[cfg(test)]
thread_local! {
    /// Ruta fija para las pruebas: así no se toca la configuración de verdad
    /// de quien las corre.
    #[allow(clippy::missing_const_for_thread_local)]
    static RUTA_FIJA: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
}

/// Fija dónde se guarda la apariencia durante las pruebas.
#[cfg(test)]
pub fn fijar_ruta(ruta: Option<std::path::PathBuf>) {
    RUTA_FIJA.with(|r| *r.borrow_mut() = ruta);
}

/// Dónde se guarda la apariencia. En Windows va en `%APPDATA%`; en Linux y
/// macOS, en `~/.config`. No se usa ninguna biblioteca externa: se lee y se
/// escribe a mano para no cargar el arranque ni el binario.
fn ruta() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    if let Some(fija) = RUTA_FIJA.with(|r| r.borrow().clone()) {
        return Some(fija);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(std::path::PathBuf::from)
    } else if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        Some(std::path::PathBuf::from(xdg))
    } else {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
    }?;
    Some(base.join("punto-de-venta").join("apariencia.conf"))
}

impl Apariencia {
    /// Lee las preferencias del disco. Si no hay archivo (o está escrito a
    /// mano y mal), devuelve las de siempre: la apariencia nunca puede
    /// impedir abrir el programa.
    pub fn cargar() -> Self {
        let Some(ruta) = ruta() else {
            return Self::default();
        };
        let Ok(texto) = std::fs::read_to_string(&ruta) else {
            return Self::default();
        };
        let mut ap = Self::default();
        for linea in texto.lines() {
            let Some((clave, valor)) = linea.split_once('=') else {
                continue;
            };
            let valor = valor.trim();
            match clave.trim() {
                "tema" => {
                    ap.tema = Tema::TODOS
                        .into_iter()
                        .find(|t| t.nombre().eq_ignore_ascii_case(valor))
                        .unwrap_or(ap.tema);
                }
                "acento" => ap.acento = leer_color(valor),
                "letra" => {
                    ap.letra = Letra::TODAS
                        .into_iter()
                        .find(|l| l.nombre().eq_ignore_ascii_case(valor))
                        .unwrap_or(ap.letra);
                }
                "densidad" => {
                    ap.densidad = Densidad::TODAS
                        .into_iter()
                        .find(|d| d.nombre().eq_ignore_ascii_case(valor))
                        .unwrap_or(ap.densidad);
                }
                "redondeo" => {
                    ap.redondeo = Redondeo::TODOS
                        .into_iter()
                        .find(|r| r.nombre().eq_ignore_ascii_case(valor))
                        .unwrap_or(ap.redondeo);
                }
                "bordes" => ap.bordes_marcados = valor == "si",
                // `barra_ancha` es el nombre viejo: se sigue leyendo para no
                // perder la preferencia de quien ya tenía el archivo.
                "barra_compacta" | "barra_ancha" => ap.barra_compacta = valor == "si",
                _ => {}
            }
        }
        ap
    }

    /// Escribe las preferencias. Si no se puede (disco lleno, permisos), no
    /// pasa nada: se pierde la personalización, no la venta.
    pub fn guardar(&self) -> Result<(), String> {
        let Some(ruta) = ruta() else {
            return Err("No encontré dónde guardar la configuración.".into());
        };
        if let Some(padre) = ruta.parent() {
            std::fs::create_dir_all(padre).map_err(|e| e.to_string())?;
        }
        let acento = match self.acento {
            Some([r, g, b]) => format!("#{r:02X}{g:02X}{b:02X}"),
            None => "tema".to_string(),
        };
        let contenido = format!(
            "# Apariencia del punto de venta. Se puede editar a mano.\n\
             tema = {}\n\
             acento = {acento}\n\
             letra = {}\n\
             densidad = {}\n\
             redondeo = {}\n\
             bordes = {}\n\
             barra_compacta = {}\n",
            self.tema.nombre(),
            self.letra.nombre(),
            self.densidad.nombre(),
            self.redondeo.nombre(),
            if self.bordes_marcados { "si" } else { "no" },
            if self.barra_compacta { "si" } else { "no" },
        );
        std::fs::write(&ruta, contenido).map_err(|e| e.to_string())
    }

    /// Vuelve a lo de siempre y lo guarda.
    pub fn restablecer(&mut self) {
        *self = Self::default();
        let _ = self.guardar();
    }
}

/// Lee `#3DDC97`. Si está mal escrito, `None` (y se usa el acento del tema).
fn leer_color(valor: &str) -> Option<[u8; 3]> {
    let hex = valor.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r, g, b])
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn las_iniciales_distinguen_productos_parecidos() {
        assert_eq!(iniciales_de("Producto número 0 de la tienda"), "P0");
        assert_eq!(iniciales_de("Producto número 1 de la tienda"), "P1");
        assert_eq!(iniciales_de("Producto número 10 de la tienda"), "10");
        assert_eq!(iniciales_de("Producto número 11 de la tienda"), "11");
        assert_eq!(iniciales_de("Coca-Cola 600 ml"), "C6");
        assert_eq!(iniciales_de("Pan blanco Bimbo"), "PB");
        assert_eq!(iniciales_de("Ana"), "AN");
        assert_eq!(iniciales_de("Administrador"), "AD");
        assert_ne!(
            color_de_etiqueta("Coca-Cola 600 ml"),
            color_de_etiqueta("Pan blanco Bimbo")
        );
    }

    #[test]
    fn las_preferencias_dan_la_vuelta_completa() {
        // Se guardan en texto, se vuelven a leer y son las mismas. En una
        // máquina sin HOME (o sin permisos) se usa el valor por defecto y
        // nadie se entera: esto no puede tumbar el programa.
        let ap = Apariencia {
            tema: Tema::Cafe,
            acento: Some([0x12, 0x34, 0x56]),
            letra: Letra::Grande,
            densidad: Densidad::Compacta,
            redondeo: Redondeo::Cuadrado,
            bordes_marcados: true,
            barra_compacta: true,
        };
        // El archivo se escribe sin importar el entorno; si no hay dónde,
        // `guardar` avisa y seguimos.
        let _ = ap.guardar();
        // Lo que sí se prueba es el viaje por texto, que es donde se puede
        // perder un dato.
        assert_eq!(leer_color("#3DDC97"), Some([0x3D, 0xDC, 0x97]));
        assert_eq!(leer_color("3DDC97"), None, "sin # no es un color");
        assert_eq!(leer_color("#3DDC9"), None, "incompleto no es un color");
        assert_eq!(leer_color("#ZZZZZZ"), None, "basura no es un color");
    }

    #[test]
    fn el_acento_elegido_manda_sobre_el_del_tema() {
        let mut ap = Apariencia::por_defecto();
        assert_eq!(ap.acento_efectivo(), Tema::POR_DEFECTO.paleta().acento);
        ap.acento = Some([0xFF, 0x00, 0x00]);
        assert_eq!(ap.acento_efectivo(), c(0xFF, 0x00, 0x00));
        // Y el tenue se deriva de él, no se queda con el verde del tema.
        assert_ne!(
            ap.paleta().acento_tenue,
            Tema::POR_DEFECTO.paleta().acento_tenue
        );
    }

    /// Los cuatro colores que significan algo (marca, peligro, aviso, todo
    /// bien) tienen que distinguirse entre ellos. Si dos salen iguales —como
    /// cuando el "éxito" era el mismo verde del acento— el color deja de
    /// decir nada y la interfaz se vuelve ruido.
    #[test]
    fn los_colores_que_significan_algo_se_distinguen() {
        for tema in Tema::TODOS {
            // El alto contraste es la excepción: ahí manda la legibilidad y
            // el amarillo se usa para las dos cosas a propósito.
            if matches!(tema, Tema::AltoContraste) {
                continue;
            }
            let p = tema.paleta();
            let cuatro = [p.acento, p.peligro, p.advertencia, p.exito];
            for i in 0..cuatro.len() {
                for j in (i + 1)..cuatro.len() {
                    let a = cuatro[i];
                    let b = cuatro[j];
                    let distancia = ((a.r() as f32 - b.r() as f32).powi(2)
                        + (a.g() as f32 - b.g() as f32).powi(2)
                        + (a.b() as f32 - b.b() as f32).powi(2))
                    .sqrt();
                    assert!(
                        distancia > 60.0,
                        "{}: dos colores con significado salen casi iguales \
                         (distancia {distancia:.0}): {a:?} y {b:?}",
                        tema.nombre()
                    );
                }
            }
        }
    }

    #[test]
    fn todos_los_temas_se_leen() {
        // Un tema claro con texto claro sería ilegible. Se revisa el
        // contraste a ojo de máquina: la diferencia de luminancia tiene que
        // ser grande en los cinco.
        for tema in Tema::TODOS {
            let p = tema.paleta();
            let luminancia = |c: egui::Color32| {
                let f = |v: u8| {
                    let v = v as f32 / 255.0;
                    if v <= 0.03928 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                };
                0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
            };
            let (l1, l2) = (luminancia(p.texto), luminancia(p.panel));
            let contraste = (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05);
            assert!(
                contraste >= 4.5,
                "{}: contraste de {contraste:.1} es poco para leer ({:?})",
                tema.nombre(),
                (p.texto, p.panel)
            );
        }
    }

    #[test]
    fn la_letra_grande_hace_todo_mas_grande() {
        let chica = Letra::Chica.factor();
        let extra = Letra::Extra.factor();
        assert!(chica < 1.0 && extra > 1.0);
        // Y la densidad suelta aire en orden.
        assert!(Densidad::Compacta.alto_renglon() < Densidad::Comoda.alto_renglon());
        let (compacta, _) = Densidad::Compacta.medidas();
        let (comoda, _) = Densidad::Comoda.medidas();
        assert!(compacta.y < comoda.y);
    }

    #[test]
    fn columnas_nunca_estallan_con_ancho_infinito() {
        for w in [
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            -10.0,
            0.0,
            1.0e20,
            640.0,
            2100.0,
            100.0,
        ] {
            let n = cuantas_columnas_de(w, 210.0);
            assert!(
                (1..=8).contains(&n),
                "ancho {w} produjo {n} columnas (debe ser 1..=8, no usize::MAX)"
            );
        }
        assert_eq!(cuantas_columnas_de(640.0, 210.0), 3);
        assert_eq!(cuantas_columnas_de(2100.0, 210.0), 8); // tope
        assert_eq!(cuantas_columnas_de(100.0, 210.0), 1); // piso
    }

    #[test]
    fn aplicar_un_tema_cambia_los_colores_que_ve_la_interfaz() {
        // Es la razón de que los colores sean funciones y no constantes:
        // cambiar de tema se ve en toda la aplicación sin tocar las
        // pantallas. (Va en hilo aparte porque la paleta es por hilo.)
        std::thread::spawn(|| {
            assert_eq!(acento(), Tema::POR_DEFECTO.paleta().acento);
            let ap = Apariencia {
                tema: Tema::Oceano,
                ..Apariencia::por_defecto()
            };
            PALETA.with(|p| *p.borrow_mut() = ap.paleta());
            assert_eq!(acento(), Tema::Oceano.paleta().acento);
            assert_eq!(texto(), Tema::Oceano.paleta().texto);
        })
        .join()
        .unwrap();
    }
}
