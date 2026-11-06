//! Cada pantalla, dibujada con una tienda del tamaño de una de verdad.
//!
//! La mayoría de los errores de interfaz no salen con cuatro productos de
//! prueba: salen cuando el catálogo tiene cinco mil, cuando el reporte trae mil
//! ventas o cuando la bitácora tiene doscientos renglones. Estas pruebas
//! dibujan **todas** las pantallas con ese volumen y revisan que nada se caiga
//! y que lo importante se alcance a ver.
//!
//! No verifican que se vea bonito (para eso están las capturas): verifican que
//! la pantalla aguante la tienda real.

#![cfg(all(test, unix))]

use pos_api::{
    CajeroDto, CategoriaDto, ClienteDto, Comando, ConfigFiscalDto, Cuerpo, EstadoCajaDto,
    EventoAuditoriaDto, EventosAuditoriaDto, FacturaDto, Peticion, PosApi, ProductoDto, Respuesta,
    TopProductoDto, UnidadDto, UsuarioDto, VentaResumenDto,
};

use crate::app::PosApp;
use crate::theme;

/// Una tienda con volumen: 5,000 productos y 1,000 ventas recientes.
struct TiendaGrande {
    productos: Vec<ProductoDto>,
    ventas: Vec<VentaResumenDto>,
}

impl TiendaGrande {
    fn nueva() -> Self {
        let productos = (0..5_000)
            .map(|i| ProductoDto {
                id: i,
                sku: format!("SKU{i:05}"),
                name: format!("Producto número {i} de la tienda"),
                price_cents: 500 + (i % 400) * 25,
                cost_cents: 250,
                tax_bp: 1600,
                stock_millis: 1_000,
                min_stock_millis: 0,
                track_inventory: true,
                clave_sat: "01010101".into(),
                clave_unidad: "H87".into(),
                unidad_sat: "Pieza".into(),
            })
            .collect();
        let ventas = (0..1_000)
            .map(|i| VentaResumenDto {
                id: i,
                folio: format!("A-{i:05}"),
                created_at: "2026-10-11 12:00:00".into(),
                total_cents: 1_800,
                status: "completada".into(),
            })
            .collect();
        TiendaGrande { productos, ventas }
    }
}

impl PosApi for TiendaGrande {
    fn ejecutar(&mut self, p: Peticion) -> Respuesta {
        match p.comando {
            // Qui'en atiende: los mismos de siempre, para que la captura del
            // inicio de sesi\u{f3}n salga como se ve en la tienda.
            Comando::UsuariosLogin => Respuesta::Ok(Cuerpo::Usuarios(vec![
                UsuarioDto {
                    id: 1,
                    username: "admin".into(),
                    display_name: "Administrador".into(),
                    role: "Admin".into(),
                    is_active: true,
                },
                UsuarioDto {
                    id: 2,
                    username: "jefe".into(),
                    display_name: "Encargado del turno".into(),
                    role: "Supervisor".into(),
                    is_active: true,
                },
                UsuarioDto {
                    id: 3,
                    username: "ana".into(),
                    display_name: "Ana".into(),
                    role: "Cajero".into(),
                    is_active: true,
                },
                UsuarioDto {
                    id: 4,
                    username: "luis".into(),
                    display_name: "Luis".into(),
                    role: "Cajero".into(),
                    is_active: true,
                },
            ])),
            Comando::Usuarios => Respuesta::Ok(Cuerpo::Usuarios(vec![UsuarioDto {
                id: 1,
                username: "admin".into(),
                display_name: "Administrador".into(),
                role: "admin".into(),
                is_active: true,
            }])),
            Comando::BuscarProductos { limite, .. } => Respuesta::Ok(Cuerpo::Productos(
                self.productos.iter().take(limite).cloned().collect(),
            )),
            Comando::ProductosBajoMinimo => Respuesta::Ok(Cuerpo::Productos(vec![])),
            Comando::Categorias => Respuesta::Ok(Cuerpo::Categorias(vec![CategoriaDto {
                id: 1,
                name: "General".into(),
                productos: 5_000,
            }])),
            Comando::VentasRecientes { limite } => Respuesta::Ok(Cuerpo::Ventas(
                self.ventas.iter().take(limite).cloned().collect(),
            )),
            Comando::TopProductos { limite } => Respuesta::Ok(Cuerpo::Top(
                self.productos
                    .iter()
                    .take(limite)
                    .map(|p| TopProductoDto {
                        nombre: p.name.clone(),
                        cantidad_millis: 1_000,
                        importe_cents: 1_800,
                    })
                    .collect(),
            )),
            Comando::TotalesPorCajero => Respuesta::Ok(Cuerpo::Cajeros(vec![CajeroDto {
                nombre: "Ana".into(),
                tickets: 1_000,
                vendido_cents: 1_800_000,
                devuelto_cents: 0,
            }])),
            Comando::Clientes { .. } => Respuesta::Ok(Cuerpo::Clientes(vec![ClienteDto {
                id: 1,
                name: "Cliente de prueba".into(),
                phone: String::new(),
                rfc: String::new(),
                limite_cents: 0,
                saldo_cents: 0,
                disponible_cents: 0,
                regimen_fiscal: String::new(),
                codigo_postal: String::new(),
                uso_cfdi: "S01".into(),
            }])),
            Comando::Unidades => Respuesta::Ok(Cuerpo::Unidades(vec![UnidadDto {
                etiqueta: "Pieza".into(),
                ruta: "pza".into(),
            }])),
            Comando::Auditoria { limite, .. } => {
                Respuesta::Ok(Cuerpo::Auditoria(EventosAuditoriaDto {
                    eventos: (0..limite.min(200))
                        .map(|i| EventoAuditoriaDto {
                            id: i as i64,
                            usuario: "Ana".into(),
                            accion: "venta".into(),
                            etiqueta: "Venta".into(),
                            detalle: format!("venta A-{i}"),
                            cuando: "2026-10-11 12:00:00".into(),
                        })
                        .collect(),
                    acciones: vec!["venta".into()],
                    total: 1_000,
                    mostrados: limite.min(200),
                }))
            }
            Comando::ConfigFiscal => {
                Respuesta::Ok(Cuerpo::ConfigFiscal(Box::new(ConfigFiscalDto {
                    rfc: "XAXX010101000".into(),
                    nombre: "Abarrotes Lupita".into(),
                    regimen_fiscal: "626".into(),
                    codigo_postal: "91260".into(),
                    forma_pago: "01".into(),
                    periodicidad: "01".into(),
                    carpeta_salida: String::new(),
                    carpeta_entrada: String::new(),
                })))
            }
            Comando::Facturas { limite } => Respuesta::Ok(Cuerpo::Facturas(
                (0..limite.min(200))
                    .map(|i| FacturaDto {
                        id: i as i64,
                        folio: format!("G202610{i}-01"),
                        tipo: "global".into(),
                        periodo: "2026-10-11".into(),
                        periodicidad: "01".into(),
                        receptor_rfc: "XAXX010101000".into(),
                        subtotal_cents: 10_000,
                        iva_cents: 1_600,
                        total_cents: 11_600,
                        ventas: "A-1".into(),
                        estado: "pendiente".into(),
                        uuid: None,
                        ultimo_error: None,
                        intentos: 0,
                        created_at: "2026-10-11 12:00:00".into(),
                        timbrada_at: None,
                    })
                    .collect(),
            )),
            Comando::EstadoCaja => Respuesta::Ok(Cuerpo::Caja(EstadoCajaDto {
                abierta: true,
                sesion_id: Some(1),
                fondo_cents: 50_000,
                ventas_efectivo_cents: 180_000,
                entradas_cents: 0,
                retiros_cents: 20_000,
                gastos_cents: 5_000,
                esperado_cents: 205_000,
                tickets: 42,
                vendido_cents: 232_000,
            })),
            _ => Respuesta::Ok(Cuerpo::Vacio),
        }
    }
}

/// Deja la aplicación en la pantalla indicada, con los datos ya cargados
/// (igual que cuando se le pica al botón de la barra de arriba).
fn en_pantalla(cual: crate::Pantalla) -> PosApp {
    let mut app = PosApp::new(Box::new(TiendaGrande::nueva()), false);
    app.pantalla = cual;
    match cual {
        crate::Pantalla::Clientes => app.refrescar_clientes(),
        crate::Pantalla::Pedidos => app.refrescar_pedidos(),
        crate::Pantalla::Reportes => app.refrescar_cajeros(),
        crate::Pantalla::Caja => app.refrescar_caja(),
        crate::Pantalla::Conteo => app.refrescar_conteo(),
        crate::Pantalla::Auditoria => app.refrescar_auditoria(),
        crate::Pantalla::Config => app.refrescar_fiscal(),
        _ => {}
    }
    app
}

/// Las nueve pantallas, dibujadas una por una con la tienda grande.
///
/// Lo que se busca aquí no es que se vean bien: es que **ninguna truene** con
/// cinco mil productos cargados. Un `unwrap`, un índice fuera de rango o un
/// desborde se ven aquí, no en la tienda.
#[test]
fn todas_las_pantallas_aguantan_una_tienda_de_cinco_mil_productos() {
    for cual in [
        crate::Pantalla::Login,
        crate::Pantalla::Venta,
        crate::Pantalla::Catalogo,
        crate::Pantalla::Pedidos,
        crate::Pantalla::Clientes,
        crate::Pantalla::Reportes,
        crate::Pantalla::Caja,
        crate::Pantalla::Conteo,
        crate::Pantalla::Auditoria,
        crate::Pantalla::Config,
    ] {
        let mut app = en_pantalla(cual);
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::Vec2::new(1400.0, 900.0))
            .build(|ctx| {
                // El tema se aplica de verdad: si una pantalla pide un color
                // que no existe, salta aquí y no frente al cliente.
                theme::aplicar(ctx);
                egui::CentralPanel::default().show(ctx, |ui| match app.pantalla {
                    crate::Pantalla::Login => app.pantalla_login(ui),
                    crate::Pantalla::Catalogo => app.pantalla_catalogo(ui),
                    crate::Pantalla::Pedidos => app.pantalla_pedidos(ui),
                    crate::Pantalla::Clientes => app.pantalla_clientes(ui),
                    crate::Pantalla::Reportes => app.pantalla_reportes(ui),
                    crate::Pantalla::Caja => app.pantalla_caja(ui),
                    crate::Pantalla::Conteo => app.pantalla_conteo(ui),
                    crate::Pantalla::Auditoria => app.pantalla_auditoria(ui),
                    crate::Pantalla::Config => app.pantalla_config(ui),
                    crate::Pantalla::Venta => app.pantalla_venta(ctx),
                });
            });
        h.run();
        // Si llega aquí, la pantalla se dibujó completa.
    }
}

#[test]
fn el_catalogo_muestra_los_productos_que_la_api_mando() {
    let mut app = en_pantalla(crate::Pantalla::Catalogo);
    app.cargar_catalogo();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1400.0, 900.0))
        .build_ui(|ui| app.pantalla_catalogo(ui));
    h.run();
    use egui_kittest::kittest::Queryable;
    assert!(
        h.query_by_label("Producto número 0 de la tienda").is_some(),
        "no se ve el primer producto del catálogo"
    );
}

#[test]
fn los_reportes_se_dibujan_con_mil_ventas() {
    let mut app = en_pantalla(crate::Pantalla::Reportes);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1400.0, 900.0))
        .build_ui(|ui| app.pantalla_reportes(ui));
    h.run();
    use egui_kittest::kittest::Queryable;
    assert!(
        h.query_by_label("Ana").is_some(),
        "no se ve el reporte por cajero"
    );
}

#[test]
fn la_auditoria_se_dibuja_con_doscientos_eventos() {
    let mut app = en_pantalla(crate::Pantalla::Auditoria);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1400.0, 900.0))
        .build_ui(|ui| app.pantalla_auditoria(ui));
    h.run();
    use egui_kittest::kittest::Queryable;
    // La etiqueta ("Venta") y el detalle ("venta A-0") van en dos textos; el
    // detalle no se repite, así que es el que se puede buscar sin ambigüedad.
    assert!(
        h.query_by_label("venta A-0").is_some(),
        "la bitácora no muestra los eventos"
    );
}

// ------------------------------------------------------------------ capturas --
//
// Una imagen por pantalla, para verlas con los propios ojos. No verifican
// nada: existen para que quien usa el programa pueda revisar la interfaz sin
// tener que instalarlo primero.

/// Dibuja una pantalla y la guarda como PNG en `tests/snapshots/`.
///
/// El tamaño es el de una netbook de las que se usen en un mostrador (1366×768)
/// para que se vea lo que de veras cabe, no lo que cabría en un monitor grande.
macro_rules! captura {
    ($h:ident, $nombre:expr) => {{
        $h.run();
        $h.snapshot($nombre);
    }};
}

/// La barra de arriba, dibujada de verdad: es la navegación de todo el
/// programa y en las otras capturas no sale.
#[test]
fn las_pestanas_llevan_a_la_pantalla_y_cargan_lo_de_esa_pantalla() {
    use egui_kittest::kittest::Queryable;

    let mut app = en_pantalla(crate::Pantalla::Venta);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1366.0, 300.0))
        .build(|ctx| {
            theme::aplicar(ctx);
            app.barra_superior(ctx);
        });
    h.run();
    // En dos filas, cada pestaña dice su atajo: se aprende sin manual.
    h.get_by_label("Reportes  F8").click();
    h.run();
    drop(h);

    assert_eq!(
        app.pantalla,
        crate::Pantalla::Reportes,
        "la pestaña cambió de pantalla"
    );
    assert!(
        !app.cajeros.is_empty(),
        "y cargó lo que esa pantalla necesita, como lo haría el atajo F8"
    );
}

/// Con la barra en una sola fila las pestañas se ven sin el atajo, que
/// queda como pista al pasar el mouse.
#[test]
fn en_barra_compacta_las_pestanas_siguen_funcionando() {
    use egui_kittest::kittest::Queryable;

    let mut app = en_pantalla(crate::Pantalla::Venta);
    app.apariencia.barra_compacta = true;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1366.0, 300.0))
        .build(|ctx| {
            theme::aplicar(ctx);
            app.barra_superior(ctx);
        });
    h.run();
    h.get_by_label("Caja").click();
    h.run();
    drop(h);
    assert_eq!(app.pantalla, crate::Pantalla::Caja);
}

#[test]
fn captura_de_cada_pantalla() {
    // Hora fija: la barra de arriba muestra el reloj, y si corriera de verdad
    // ninguna captura sería igual a la anterior.
    crate::app::fijar_hora(Some("2026-10-11 09:35"));
    use crate::Pantalla;
    for cual in [
        Pantalla::Login,
        Pantalla::Venta,
        Pantalla::Catalogo,
        Pantalla::Clientes,
        Pantalla::Reportes,
        Pantalla::Caja,
        Pantalla::Conteo,
        Pantalla::Auditoria,
        Pantalla::Config,
    ] {
        let nombre = match cual {
            Pantalla::Login => "01_login",
            Pantalla::Venta => "02_venta",
            Pantalla::Catalogo => "03_catalogo",
            Pantalla::Pedidos => "10_pedidos",
            Pantalla::Clientes => "04_clientes",
            Pantalla::Reportes => "05_reportes",
            Pantalla::Caja => "06_caja",
            Pantalla::Conteo => "07_conteo",
            Pantalla::Auditoria => "08_auditoria",
            Pantalla::Config => "09_configuracion",
        };
        let mut app = en_pantalla(cual);
        match cual {
            Pantalla::Catalogo => app.cargar_catalogo(),
            Pantalla::Caja => app.refrescar_caja(),
            Pantalla::Venta => {
                // Con productos a la vista y dos en el ticket: una captura
                // con la pantalla vacía no enseña nada.
                app.cargar_catalogo();
                let muestra: Vec<_> = app.resultados.iter().take(12).cloned().collect();
                app.resultados = muestra.clone();
                for p in muestra.iter().take(2) {
                    app.agregar_producto(p);
                }
            }
            _ => {}
        }
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::Vec2::new(1366.0, 768.0))
            .build(|ctx| {
                theme::aplicar(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    if app.pantalla == Pantalla::Venta {
                        app.pantalla_venta(ctx);
                    } else {
                        app.cuerpo_de_la_pantalla(ui);
                    }
                });
            });
        captura!(h, nombre);
    }
}

/// La ventana como se ve de verdad: barra de arriba, ticket y búsqueda.
/// Las otras capturas dibujan sólo el Centro, sin la navegación.
#[test]
fn captura_de_la_ventana_completa() {
    crate::app::fijar_hora(Some("2026-10-11 09:35"));
    let mut app = en_pantalla(crate::Pantalla::Venta);
    app.usuario = Some(UsuarioDto {
        id: 1,
        username: "ana".into(),
        display_name: "Ana".into(),
        role: "Cajero".into(),
        is_active: true,
    });
    app.cargar_catalogo();
    let muestra: Vec<_> = app.resultados.iter().take(12).cloned().collect();
    app.resultados = muestra.clone();
    for p in muestra.iter().take(2) {
        app.agregar_producto(p);
    }
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1366.0, 768.0))
        .build(|ctx| {
            theme::aplicar(ctx);
            app.barra_superior(ctx);
            app.pantalla_venta(ctx);
        });
    captura!(h, "11_ventana_completa");
}

/// Los cinco temas, dibujados con la misma pantalla. No es una captura de
/// prueba: existe para poder elegir cuál se queda.
#[test]
fn captura_de_los_cinco_temas() {
    use crate::theme::{Apariencia, Tema};
    crate::app::fijar_hora(Some("2026-10-11 09:35"));
    let dir = std::env::temp_dir().join(format!("pos-temas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    for tema in Tema::TODOS {
        let apariencia = Apariencia {
            tema,
            ..Apariencia::por_defecto()
        };
        let mut app = en_pantalla(crate::Pantalla::Venta);
        app.apariencia = apariencia.clone();
        app.cargar_catalogo();
        let muestra: Vec<_> = app.resultados.iter().take(8).cloned().collect();
        app.resultados = muestra.clone();
        for p in muestra.iter().take(2) {
            app.agregar_producto(p);
        }
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::Vec2::new(1280.0, 620.0))
            .build(|ctx| {
                theme::aplicar_con(ctx, &apariencia);
                app.barra_superior(ctx);
                app.pantalla_venta(ctx);
            });
        h.run();
        let imagen = h.render().unwrap();
        let ruta = dir.join(format!(
            "{}.png",
            tema.nombre().to_lowercase().replace(' ', "_")
        ));
        imagen.save(&ruta).unwrap();
    }
    println!("TEMAS EN {}", dir.display());
}

/// La sección desde la que se cambia el tema, la letra y el color.
#[test]
fn captura_de_la_apariencia() {
    let mut app = en_pantalla(crate::Pantalla::Config);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1100.0, 1400.0))
        .build_ui(|ui| {
            // Un trozo de la pantalla de configuración: la sección nueva.
            app.seccion_apariencia(ui);
        });
    captura!(h, "12_apariencia");
}

/// La ventana de cobro: donde el cajero vive la mitad del día.
#[test]
fn captura_de_la_ventana_de_cobro() {
    crate::app::fijar_hora(Some("2026-10-11 09:35"));
    let mut app = en_pantalla(crate::Pantalla::Venta);
    // Una venta de tres cosas, como cualquier ticket.
    for (sku, precio) in [
        ("SKU00000", 5_000i64),
        ("SKU00001", 5_025),
        ("SKU00002", 5_050),
    ] {
        app.agregar_producto(&ProductoDto {
            id: 0,
            sku: sku.into(),
            name: format!("Producto {sku}"),
            price_cents: precio,
            cost_cents: precio / 2,
            tax_bp: 1600,
            stock_millis: 1_000,
            min_stock_millis: 0,
            track_inventory: true,
            clave_sat: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad_sat: "Pieza".into(),
        });
    }
    app.mostrar_cobro = true;
    app.abrir_cobro();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1366.0, 768.0))
        .build(|ctx| {
            theme::aplicar(ctx);
            egui::CentralPanel::default().show(ctx, |_ui| app.pantalla_venta(ctx));
            // La ventana de cobro vive aparte de la pantalla de venta: si no
            // se pide, la captura sale vacía a la izquierda.
            app.ventana_cobro(ctx, false);
        });
    captura!(h, "10_cobro");
}
