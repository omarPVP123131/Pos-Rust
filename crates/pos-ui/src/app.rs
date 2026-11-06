//! Pantalla principal del punto de venta.
//!
//! **Regla de este archivo:** aquí sólo se dibuja y se llama a la API.
//! Ni una consulta, ni un impuesto, ni un movimiento de inventario: todo eso
//! vive en `pos-api`. Si este archivo se reescribe en otro toolkit, el sistema
//! sigue funcionando igual.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;

use pos_api::{
    AnalisisCsvDto, CajeroDto, CategoriaDto, ClienteDto, Comando, ConfigDto, ConfigFiscalDto,
    ConteoDto, Cuerpo, EstadoActualizacionDto, EstadoCajaDto, EstadoRespaldosDto,
    EventosAuditoriaDto, FacturaDto, ImpresoraDto, LineaDto, NuevoClienteDto, PagoDto, PedidoDto,
    Peticion, PosApi, ProductoDto, ProveedorDto, ResultadoImporte, TicketDto, UnidadDto,
    UsuarioDto, VentaDto, DENOMINACIONES_MXN,
};
use pos_core::{FechaHora, Money, PaymentMethod, Quantity, Sale, SaleLine, TaxRate};
use pos_hw::{ReceiptData, ReceiptItem};

use crate::iconos::Icono;
use crate::theme;

/// Etiqueta en español de una acción de la bitácora. Es la misma tabla que
/// usa el API; se repite aquí porque la interfaz no puede importar `pos-db`.
fn etiqueta_accion(accion: &str) -> String {
    String::from(match accion {
        "venta" => "Venta",
        "cancelacion" => "Devolución",
        "alta" => "Producto nuevo",
        "edicion" => "Producto editado",
        "baja" => "Producto dado de baja",
        "ajuste_inventario" => "Ajuste de inventario",
        "proveedor" => "Proveedor",
        "pedido" => "Pedido a proveedor",
        "recibir_pedido" => "Recibió un pedido",
        "cancelar_pedido" => "Canceló un pedido",
        "importacion" => "Importación de catálogo",
        "abrir_caja" => "Abrió caja",
        "cerrar_caja" => "Cortó caja",
        "movimiento_caja" => "Movimiento de caja",
        "login" => "Entró al sistema",
        "login_fallido" => "PIN equivocado",
        "bloqueo" => "Usuario bloqueado",
        "abono" => "Abono a cartera",
        "autorizar" => "Autorización",
        "precio" => "Precio a mano",
        "descuento" => "Descuento",
        "fiado" => "Fiado",
        "diferencia" => "Corte que no cuadra",
        "retiro" => "Retiro o gasto",
        "pac_http" => "PAC por HTTP bloqueado",
        "impresion_prueba" => "Ticket de prueba",
        "impresion_prueba_fallida" => "Ticket de prueba fallido",
        "respaldo" => "Respaldo",
        "configuracion" => "Cambió la configuración",
        "iniciar_conteo" => "Empezó un conteo físico",
        "cerrar_conteo" => "Cerró un conteo físico",
        conocida => conocida,
    })
}

/// Qué icono y qué color lleva cada cosa que pasó en el sistema.
///
/// Sirve para lo mismo que los colores del semáforo: barrer la bitácora con la
/// mirada y que lo que salió mal (autorizaciones, devoluciones, bloqueos) se
/// distinga de lo que fue trabajo normal.
fn icono_de_accion(accion: &str) -> (Icono, egui::Color32) {
    match accion {
        "venta" => (Icono::Ticket, theme::exito()),
        "cancelacion" => (Icono::Basura, theme::peligro()),
        "autorizar" => (Icono::Alerta, theme::advertencia()),
        "precio" => (Icono::Etiqueta, theme::advertencia()),
        "descuento" => (Icono::Etiqueta, theme::advertencia()),
        "fiado" => (Icono::Tarjeta, theme::advertencia()),
        "diferencia" => (Icono::Alerta, theme::peligro()),
        "retiro" => (Icono::Billete, theme::advertencia()),
        "pac_http" => (Icono::Alerta, theme::peligro()),
        "impresion_prueba" => (Icono::Impresora, theme::exito()),
        "impresion_prueba_fallida" => (Icono::Impresora, theme::peligro()),
        "abrir_caja" | "cerrar_caja" | "movimiento_caja" => (Icono::Billete, theme::acento()),
        "login" => (Icono::Persona, theme::texto_suave()),
        "login_fallido" | "bloqueo" => (Icono::Candado, theme::peligro()),
        "abono" => (Icono::Tarjeta, theme::exito()),
        "alta" | "edicion" | "baja" | "ajuste_inventario" | "importacion" | "proveedor"
        | "pedido" | "recibir_pedido" | "cancelar_pedido" => {
            (Icono::CajaProducto, theme::texto_suave())
        }
        "iniciar_conteo" | "cerrar_conteo" => (Icono::Portapapeles, theme::acento()),
        "respaldo" => (Icono::Escudo, theme::exito()),
        "configuracion" => (Icono::Engranaje, theme::texto_suave()),
        _ => (Icono::Libro, theme::texto_suave()),
    }
}

/// Tamaño legible para el cajero: "148 KB", "2.3 MB".
fn tamano(bytes: u64) -> String {
    if bytes < 1_024 {
        format!("{bytes} B")
    } else if bytes < 1_024 * 1_024 {
        format!("{:.0} KB", bytes as f64 / 1_024.0)
    } else if bytes < 1_024 * 1_024 * 1_024 {
        format!("{:.1} MB", bytes as f64 / (1_024.0 * 1_024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1_024.0 * 1_024.0 * 1_024.0))
    }
}

// La facturación vive en su propio archivo porque esta pantalla ya pasa de
// cuatro mil líneas. Es submódulo de `app` (y no hermano) porque necesita ver
// los campos de PosApp, que son privados de aquí hacia afuera.
#[path = "facturacion.rs"]
mod facturacion;
#[path = "pantallas_prueba.rs"]
mod pantallas_prueba;
#[path = "pedidos.rs"]
mod pedidos;
#[path = "usuarios.rs"]
mod usuarios;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pantalla {
    Login,
    Venta,
    Catalogo,
    Pedidos,
    Clientes,
    Reportes,
    Caja,
    Conteo,
    Config,
    Auditoria,
}

/// Una línea de la ventana de cobro: permite pago mixto (efectivo + tarjeta).
#[derive(Clone)]
struct FilaPago {
    metodo: PaymentMethod,
    monto: String,
    /// Sólo importa en efectivo: con cuánto paga el cliente.
    recibido: String,
}

/// La última cosa que pasó en la venta, para la barra de feedback.
///
/// Guarda la venta anterior para que "Deshacer" sea de verdad: no basta con
/// borrar la última línea si lo que se hizo fue cambiar una cantidad.
#[derive(Clone)]
struct FeedbackVenta {
    titulo: String,
    detalle: String,
    sku: Option<String>,
    antes: Option<Sale>,
}

#[derive(Default, Clone)]
struct FormProducto {
    sku: String,
    nombre: String,
    precio: String,
    costo: String,
    codigo: String,
    stock: String,
    categoria: String,
    /// Claves del catálogo del SAT. Vacías = el producto usa las genéricas.
    clave_sat: String,
    clave_unidad: String,
    unidad_sat: String,
}

pub struct PosApp {
    /// Toda la operación pasa por aquí. La interfaz no sabe nada más.
    api: Box<dyn PosApi>,
    pantalla: Pantalla,

    // --- sesión ---
    token: Option<String>,
    usuario: Option<UsuarioDto>,
    usuarios: Vec<UsuarioDto>,
    form_usu_id: Option<i64>,
    form_usu_username: String,
    form_usu_nombre: String,
    form_usu_pin: String,
    form_usu_rol: String,
    pin: String,
    pin_error: String,
    pendiente: Option<UsuarioDto>,

    // --- venta en curso ---
    venta: Sale,
    resultados: Vec<ProductoDto>,
    consulta: String,
    /// Cuántas piezas se agregan al escanear o picar una baldosa. "1" al arrancar.
    venta_cant: String,
    /// Los últimos productos que se picaron: atajos del turno, sin guardar.
    recientes: Vec<(String, String)>,
    /// Alta de producto plegada: la lista es lo primero que se busca.
    mostrar_alta_producto: bool,
    filtro_catalogo: String,
    /// Alta de cliente plegada: primero la cartera.
    mostrar_alta_cliente: bool,
    filtro_clientes: String,
    foco_busqueda: bool,
    mostrar_cobro: bool,
    /// Si la sección de descuento y fiado está desplegada.
    descuento_abierto: bool,
    /// Filtro de la tabla de venta: todo | recientes | se-agota.
    filtro_venta: String,
    /// Producto resaltado en la tabla. Enter lo agrega sin usar el ratón.
    producto_sel: Option<String>,
    /// Última acción de la venta y el estado al que se puede volver.
    feedback_venta: Option<FeedbackVenta>,
    /// Uno o más pagos; su suma debe cubrir el total.
    pagos: Vec<FilaPago>,
    descuento_linea: String,
    descuento_global: String,
    cliente_id: Option<i64>,
    cliente_nombre: String,
    clientes: Vec<ClienteDto>,

    // --- caja ---
    caja: EstadoCajaDto,
    abrir_caja_monto: String,
    mostrar_abrir_caja: bool,
    /// Tras el PIN: no cargar catálogo/caja en el mismo frame del clic.
    /// El primer pintado de Venta espera al frame siguiente, ya con
    /// paneles estables (en Windows el frame del clic llegaba con anchos inf).
    cargar_tras_entrar: bool,
    mostrar_arqueo: bool,
    arqueo: Vec<(i64, i64)>,
    mov_tipo: String,
    mov_monto: String,
    mov_motivo: String,
    ultimo_cierre: Option<(i64, i64, i64)>, // esperado, contado, diferencia

    // --- clientes ---
    form_cliente: NuevoClienteDto,
    form_cliente_limite: String,
    abono_monto: String,
    cliente_abono: Option<i64>,

    // --- reportes ---
    cajeros: Vec<CajeroDto>,
    desde: String,
    hasta: String,
    folio_buscar: String,
    pub(crate) mensaje: String,
    vista_previa: Option<String>,

    /// Línea del ticket cuyo precio se está editando. Queda en la bitácora.
    precio_linea: Option<usize>,
    precio_linea_txt: String,
    /// Línea cuya cantidad se está escribiendo (en vez de picar + doce veces).
    cantidad_linea: Option<usize>,
    cantidad_linea_txt: String,
    /// Productos en mínimo, para la alerta del mostrador.
    bajo_minimo: Vec<ProductoDto>,
    ruta_respaldo: String,
    respaldos: EstadoRespaldosDto,
    unidades: Vec<UnidadDto>,
    // --- auditoría ---
    bitacora: EventosAuditoriaDto,
    aud_desde: String,
    aud_hasta: String,
    aud_usuario: String,
    aud_accion: String,
    aud_texto: String,

    // --- catálogo: importación CSV e inventario ---
    editando_id: Option<i64>,
    ruta_csv: String,
    csv_actualizar: bool,
    ultimo_importe: Option<ResultadoImporte>,
    categorias: Vec<CategoriaDto>,
    categoria_nueva: String,
    inv_producto: Option<i64>,
    inv_cantidad: String,
    inv_motivo: String,
    // --- alta rápida: 3 campos para el comerciante común ---
    // Nombre, precio y cuántos hay. El SKU/código se genera solo si no se
    // escribe; el resto (categoría, SAT, mínimo) va en "Modo avanzado".
    alta_nombre: String,
    alta_precio: String,
    alta_stock: String,
    alta_costo: String,
    alta_codigo: String,
    alta_mensaje: String,
    alta_ok: bool,
    /// Al entrar a Catálogo el cursor cae en "¿Qué vendes?": dar de alta sin
    /// tocar el ratón (Enter guarda, ver `tarjeta_alta_rapida`).
    foco_alta: bool,

    // --- pedidos a proveedor ---
    proveedores: Vec<ProveedorDto>,
    form_proveedor_nombre: String,
    form_proveedor_telefono: String,
    pedidos: Vec<PedidoDto>,
    pedido: Option<PedidoDto>,
    pedido_proveedor: Option<i64>,
    pedido_codigo: String,
    pedido_qty: String,
    pedido_costo: String,
    pedido_entregas: Vec<String>,

    // --- conteo físico ---
    /// El conteo en curso. `id == 0` significa "no hay ninguno abierto".
    conteo: ConteoDto,
    conteo_codigo: String,
    conteo_cantidad: String,
    conteo_categoria: String,
    /// todos | por_contar | diferencia
    conteo_filtro: String,
    mostrar_firma_conteo: bool,

    // --- mapeo a mano de las columnas del CSV ---
    mostrar_mapeo: bool,
    analisis: Option<AnalisisCsvDto>,
    mapeo: Vec<String>,

    // --- actualizaciones ---
    actualizacion: EstadoActualizacionDto,
    ruta_paquete: String,
    paquete: Option<pos_api::PaqueteDto>,

    // --- impresora ---
    /// Lo que se encontró conectado la última vez que se buscó.
    impresoras: Vec<ImpresoraDto>,

    // --- facturación electrónica ---
    /// Los datos fiscales de la tienda (RFC, régimen, CP...).
    fiscal: ConfigFiscalDto,
    /// La cola de facturas, las más nuevas primero.
    facturas: Vec<FacturaDto>,
    factura_fecha: String,
    factura_periodicidad: String,
    /// Folio de la venta a la que se le hará la nota de crédito.
    nota_folio: String,
    /// Día de la nota de crédito.
    nota_fecha: String,
    /// Renglones de la venta a devolver: (sku, nombre, vendido, cuánto devuelve).
    nota_renglones: Vec<(String, String, i64, String)>,
    /// Folio de la venta que se va a facturar a nombre de su cliente.
    factura_individual_folio: String,
    /// Día de esa factura individual.
    factura_individual_fecha: String,
    /// Cuántas ventas con RFC siguen esperando su factura.
    pendientes_rfc: i64,
    /// Monto a devolver cuando la nota es por dinero y no por piezas.
    nota_monto: String,
    /// El XML que se está mostrando: (folio, contenido).
    factura_xml: Option<(String, String)>,
    /// Factura que se está por cancelar (CFDI).
    pub(crate) cancel_factura_id: Option<i64>,
    pub(crate) cancel_motivo: String,
    pub(crate) cancel_sustitucion: String,
    /// Cómo se ve el programa: tema, colores, letra, densidad. Es cosa de
    /// esta computadora, así que no pasa por la API ni se guarda en la base.
    pub(crate) apariencia: crate::theme::Apariencia,
    /// Cliente al que se le está registrando un pago de sus facturas a crédito.
    pago_cliente_id: i64,
    /// Cuánto sigue debiendo ese cliente, según la última consulta.
    pago_saldo_cents: i64,
    /// Cuánto está pagando en este momento.
    pago_monto: String,
    /// Día en que cae el pago.
    pago_fecha: String,
    /// Clave SAT de la forma en que pagó (01 efectivo, 03 transferencia...).
    pago_forma: String,

    // --- configuración local de la interfaz ---
    config: ConfigDto,
    dry_run: bool,
    form: FormProducto,
    /// Miniaturas JPEG ya subidas a la GPU. Se cargan la primera vez
    /// que un SKU estrella aparece; el resto no gasta RAM.
    fotos: std::collections::HashMap<String, egui::TextureHandle>,
}

/// Las pantallas, en el orden en que se navegan, con su atajo de teclado.
/// De aquí salen las pestañas de la barra de arriba.
///
/// Orden por uso diario: lo de todos los días primero (Venta, Catálogo,
/// Caja), lo semanal después (Clientes, Reportes, Conteo) y lo de
/// encargado/admin al final (Pedidos, Auditoría, Config). Los atajos F2..F10
/// no cambian: sólo el orden en que se barren con la mirada.
const NAV: [(Pantalla, &str, &str, Icono); 9] = [
    (Pantalla::Venta, "Venta", "F2", Icono::Ticket),
    (Pantalla::Catalogo, "Catálogo", "F3", Icono::CajaProducto),
    (Pantalla::Caja, "Caja", "F9", Icono::Billete),
    (Pantalla::Clientes, "Clientes", "F7", Icono::Persona),
    (Pantalla::Pedidos, "Pedidos", "", Icono::Camion),
    (Pantalla::Reportes, "Reportes", "F8", Icono::Grafica),
    (Pantalla::Conteo, "Conteo", "F5", Icono::Portapapeles),
    (Pantalla::Auditoria, "Auditoría", "F6", Icono::Libro),
    (Pantalla::Config, "Configuración", "F10", Icono::Engranaje),
];

/// Régimen y uso de CFDI: se eligen por nombre, no se memoriza el número.
const REGIMENES: [(&str, &str); 7] = [
    ("601", "601 · General de Ley PM"),
    ("603", "603 · Fines no lucrativos"),
    ("612", "612 · Actividad empresarial PF"),
    ("616", "616 · Sin obligaciones"),
    ("621", "621 · Incorporación Fiscal"),
    ("625", "625 · Plataformas tecnológicas"),
    ("626", "626 · RESICO"),
];
const USOS_CFDI: [(&str, &str); 5] = [
    ("G01", "G01 · Adquisición de mercancías"),
    ("G03", "G03 · Gastos en general"),
    ("S01", "S01 · Sin efectos fiscales"),
    ("CP01", "CP01 · Pagos"),
    ("D01", "D01 · Honorarios médicos"),
];

/// Una pestaña de la barra: menú de mostrador, no barra de Windows 95.
///
/// El nombre se lee; el atajo va chiquito y apagado a un lado, para no
/// pelearse con él. El lector de pantalla sigue oyendo «Reportes  F8».
/// Devuelve `true` si la picaron.
/// Una pastilla de estado con punto de color. La usan la cabecera de venta
/// y cualquier lugar que necesite decir "abierta/cerrada" sin una caja.
fn pastilla_estado(
    ui: &mut egui::Ui,
    texto: &str,
    color: egui::Color32,
    fondo: egui::Color32,
) {
    let ancho_texto = ui.fonts(|f| {
        f.layout_no_wrap(
            texto.to_string(),
            egui::FontId::proportional(10.0),
            color,
        )
        .size()
        .x
    });
    let (rect, respuesta) = ui.allocate_exact_size(
        egui::vec2(ancho_texto + 28.0, 24.0),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(12), fondo);
    ui.painter().circle_filled(
        egui::pos2(rect.left() + 11.0, rect.center().y),
        3.0,
        color,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 19.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        texto,
        egui::FontId::proportional(10.0),
        color,
    );
    respuesta.on_hover_text(texto);
}

/// Un filtro de la tabla. Activo va en sage, no en lima: el lima se reserva
/// para cobrar y para la navegación.
fn chip_venta(
    ui: &mut egui::Ui,
    rail: bool,
    activo: bool,
    texto: &str,
    cuenta: Option<usize>,
) -> bool {
    let etiqueta = match cuenta {
        Some(n) if n > 0 => format!("{texto} {n}"),
        _ => texto.to_string(),
    };
    let fuente = egui::FontId::proportional(if rail { 10.0 } else { 10.5 });
    let ancho_texto = ui.fonts(|f| {
        f.layout_no_wrap(etiqueta.clone(), fuente.clone(), theme::texto())
            .size()
            .x
    });
    let alto = if rail { 27.0 } else { 29.0 };
    let (rect, respuesta) = ui.allocate_exact_size(
        egui::vec2(ancho_texto + 22.0, alto),
        egui::Sense::click(),
    );
    let tinta = if activo {
        theme::tinta_sobre(theme::acento())
    } else {
        theme::texto_suave()
    };
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same((alto * 0.5) as u8),
        if activo {
            theme::acento()
        } else {
            theme::tarjeta()
        },
    );
    if !activo {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same((alto * 0.5) as u8),
            egui::Stroke::new(
                1.0_f32,
                if respuesta.hovered() {
                    theme::acento()
                } else {
                    theme::borde()
                },
            ),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        etiqueta,
        fuente,
        tinta,
    );
    respuesta.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), texto)
    });
    respuesta.clicked()
}

fn columnas_venta(ancho: f32, rail: bool) -> [f32; 4] {
    // Las columnas se reparten el ancho útil (sin los dos márgenes). Si no
    // suman exacto, la última queda corta y el botón de agregar se encoge.
    let pad = if rail { 12.0 } else { 16.0 };
    let accion = if rail { 82.0 } else { 100.0 };
    let precio = if rail { 72.0 } else { 92.0 };
    let existencia = if rail { 72.0 } else { 92.0 };
    let producto = (ancho - pad * 2.0 - accion - precio - existencia).max(120.0);
    [producto, precio, existencia, accion]
}

/// Escribe una etiqueta dentro de un rectángulo exacto. Se usa para que la
/// cabecera y las filas compartan la misma geometría: nada de dejar que el
/// flujo horizontal mueva las columnas por su cuenta.
fn texto_en_rect(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    texto: &str,
    tam: f32,
    color: egui::Color32,
    fuerte: bool,
    derecha: bool,
) {
    let layout = if derecha {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    let mut fui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(layout),
    );
    let mut rich = egui::RichText::new(texto).size(tam).color(color);
    if fuerte {
        rich = rich.strong();
    }
    fui.add(egui::Label::new(rich).selectable(false));
}

fn cabecera_venta(
    ui: &mut egui::Ui,
    ancho: f32,
    alto: f32,
    columnas: [f32; 4],
    rail: bool,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ancho, alto), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme::superficie_suave());
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.bottom() - 1.0),
            egui::vec2(rect.width(), 1.0),
        ),
        egui::CornerRadius::ZERO,
        theme::borde(),
    );
    let pad = if rail { 12.0 } else { 16.0 };
    let x0 = rect.left() + pad;
    let y = rect.top();
    let h = rect.height();
    let x_precio = x0 + columnas[0];
    let x_existencia = x_precio + columnas[1];
    let x_accion = x_existencia + columnas[2];
    texto_en_rect(
        ui,
        egui::Rect::from_min_size(egui::pos2(x0, y), egui::vec2(columnas[0], h)),
        "PRODUCTO",
        8.2,
        theme::texto_suave(),
        true,
        false,
    );
    texto_en_rect(
        ui,
        egui::Rect::from_min_size(egui::pos2(x_precio, y), egui::vec2(columnas[1], h)),
        "PRECIO",
        8.2,
        theme::texto_suave(),
        true,
        true,
    );
    texto_en_rect(
        ui,
        egui::Rect::from_min_size(egui::pos2(x_existencia, y), egui::vec2(columnas[2], h)),
        "EXISTENCIA",
        8.2,
        theme::texto_suave(),
        true,
        true,
    );
    texto_en_rect(
        ui,
        egui::Rect::from_min_size(egui::pos2(x_accion, y), egui::vec2(columnas[3], h)),
        "ACCIÓN",
        8.2,
        theme::texto_suave(),
        true,
        true,
    );
}

fn monograma_venta(ui: &mut egui::Ui, p: &ProductoDto, rail: bool) {
    let lado = if rail { 28.0 } else { 32.0 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(lado, lado), egui::Sense::hover());
    let base = theme::color_de_etiqueta(&p.name);
    theme::degradado(
        ui.painter(),
        rect,
        base.gamma_multiply(1.18),
        base.gamma_multiply(0.78),
        egui::CornerRadius::same(if rail { 8 } else { 9 }),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        theme::iniciales_de(&p.name),
        egui::FontId::proportional(if rail { 9.5 } else { 10.5 }),
        egui::Color32::WHITE,
    );
}

fn badge_producto(ui: &mut egui::Ui, texto: &str) {
    let fuente = egui::FontId::proportional(8.0);
    let ancho_texto = ui.fonts(|f| {
        f.layout_no_wrap(texto.to_string(), fuente.clone(), theme::acento())
            .size()
            .x
    });
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ancho_texto + 12.0, 16.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(8),
        theme::lima_tenue(),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        texto,
        fuente,
        theme::acento(),
    );
}

fn badge_piezas(ui: &mut egui::Ui, texto: &str) {
    let fuente = egui::FontId::proportional(9.0);
    let ancho_texto = ui.fonts(|f| {
        f.layout_no_wrap(texto.to_string(), fuente.clone(), theme::acento())
            .size()
            .x
    });
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ancho_texto + 16.0, 20.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(10),
        theme::lima_tenue(),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        texto,
        fuente,
        theme::acento(),
    );
}

fn fila_venta(
    ui: &mut egui::Ui,
    p: &ProductoDto,
    rail: bool,
    seleccionada: bool,
    en_ticket: i64,
    columnas: [f32; 4],
) -> (bool, bool) {
    let alto = if rail { 53.0 } else { 54.0 };
    let ancho = ui.available_width();
    let (rect, respuesta) =
        ui.allocate_exact_size(egui::vec2(ancho, alto), egui::Sense::click());

    if seleccionada {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, theme::lima_tenue());
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, alto)),
            egui::CornerRadius::ZERO,
            theme::acento(),
        );
    } else if respuesta.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::ZERO,
            theme::superficie_suave(),
        );
    }
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.bottom() - 1.0),
            egui::vec2(rect.width(), 1.0),
        ),
        egui::CornerRadius::ZERO,
        theme::tenir(theme::borde(), 0.55),
    );

    let pad = if rail { 12.0 } else { 16.0 };
    let x0 = rect.left() + pad;
    let y = rect.top();
    let x_precio = x0 + columnas[0];
    let x_existencia = x_precio + columnas[1];
    let x_accion = x_existencia + columnas[2];

    // --- producto: monograma + nombre + metadatos ---
    let lado = if rail { 28.0 } else { 32.0 };
    let mono = egui::Rect::from_center_size(
        egui::pos2(x0 + lado * 0.5, rect.center().y),
        egui::vec2(lado, lado),
    );
    let base = theme::color_de_etiqueta(&p.name);
    theme::degradado(
        ui.painter(),
        mono,
        base.gamma_multiply(1.18),
        base.gamma_multiply(0.78),
        egui::CornerRadius::same(if rail { 8 } else { 9 }),
    );
    ui.painter().text(
        mono.center(),
        egui::Align2::CENTER_CENTER,
        theme::iniciales_de(&p.name),
        egui::FontId::proportional(if rail { 9.5 } else { 10.5 }),
        egui::Color32::WHITE,
    );

    let x_texto = x0 + lado + if rail { 9.0 } else { 11.0 };
    let rect_texto = egui::Rect::from_min_max(
        egui::pos2(x_texto, rect.top() + 4.0),
        egui::pos2(x_precio - 6.0, rect.bottom() - 4.0),
    );
    let mut tui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect_texto)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    tui.spacing_mut().item_spacing = egui::vec2(4.0, 1.0);
    tui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add(
            egui::Label::new(
                egui::RichText::new(cortar(&p.name, if rail { 25 } else { 32 }))
                    .size(if rail { 11.5 } else { 12.5 })
                    .strong()
                    .color(theme::texto()),
            )
            .selectable(false)
            .truncate(),
        );
        if en_ticket > 0 {
            badge_producto(
                ui,
                &format!("{} en ticket", Quantity::from_millis(en_ticket).format()),
            );
        }
    });
    tui.add(
        egui::Label::new(
            egui::RichText::new(format!(
                "{} · {}",
                p.sku,
                if p.track_inventory {
                    "Inventario"
                } else {
                    "Sin control"
                }
            ))
            .size(if rail { 8.5 } else { 9.0 })
            .color(theme::texto_suave()),
        )
        .selectable(false)
        .truncate(),
    );

    // --- precio ---
    texto_en_rect(
        ui,
        egui::Rect::from_min_size(egui::pos2(x_precio, y), egui::vec2(columnas[1], alto)),
        &Money::from_cents(p.price_cents).format_mxn(),
        if rail { 11.5 } else { 12.5 },
        theme::acento(),
        true,
        true,
    );

    // --- existencia ---
    let bajo = p.track_inventory && p.stock_millis <= p.min_stock_millis;
    let rect_existencia =
        egui::Rect::from_min_size(egui::pos2(x_existencia, y), egui::vec2(columnas[2], alto));
    let mut eui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect_existencia)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    eui.spacing_mut().item_spacing.x = 3.0;
    eui.add(
        egui::Label::new(
            egui::RichText::new(if p.track_inventory {
                format!("{} pzas", Quantity::from_millis(p.stock_millis).format())
            } else {
                "—".to_string()
            })
            .size(if rail { 9.5 } else { 10.0 })
            .color(if bajo {
                theme::advertencia()
            } else {
                theme::texto_suave()
            }),
        )
        .selectable(false),
    );
    if bajo {
        let (r, _) = eui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
        Icono::Alerta.dibujar(eui.painter(), r.center(), 11.0, theme::advertencia());
    }

    // --- acción ---
    let ancho_boton = (columnas[3] - 4.0).min(if rail { 78.0 } else { 84.0 });
    let rect_boton = egui::Rect::from_center_size(
        egui::pos2(x_accion + columnas[3] * 0.5, rect.center().y),
        egui::vec2(ancho_boton, 25.0),
    );
    let id_boton = ui.id().with(("agregar_venta", p.sku.as_str()));
    let resp_boton = ui.interact(rect_boton, id_boton, egui::Sense::click());
    ui.painter().rect_filled(
        rect_boton,
        egui::CornerRadius::same(7),
        if seleccionada {
            theme::acento()
        } else if resp_boton.hovered() {
            theme::acento_tenue().gamma_multiply(0.94)
        } else {
            theme::acento_tenue()
        },
    );
    ui.painter().rect_stroke(
        rect_boton,
        egui::CornerRadius::same(7),
        egui::Stroke::new(
            1.0_f32,
            if seleccionada {
                theme::acento()
            } else {
                theme::tenir(theme::acento(), 0.22)
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect_boton.center(),
        egui::Align2::CENTER_CENTER,
        "+ Agregar",
        egui::FontId::proportional(if rail { 9.0 } else { 9.5 }),
        if seleccionada {
            egui::Color32::WHITE
        } else {
            theme::acento()
        },
    );
    resp_boton.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Agregar {} al ticket", p.name),
        )
    });

    respuesta.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("{} · elegir", p.name),
        )
    });
    let sumar = resp_boton.clicked() || (respuesta.double_clicked() && !resp_boton.hovered());
    (
        respuesta.clicked() && !resp_boton.hovered(),
        sumar,
    )
}

/// Desvanece el borde inferior de una lista que sigue hacia abajo.
///
/// La fila cortada se ve a media altura; en vez de un corte duro se pinta un
/// degradado opaco del color de la tarjeta. Se deja libre el canal del
/// scrollbar para no taparlo.
fn desvanecer_inferior(
    ui: &egui::Ui,
    rect: egui::Rect,
    margen_derecho: f32,
    resto: f32,
) {
    let solido = resto.max(6.0);
    let degradado = 10.0_f32.min(rect.height() * 0.15);
    let alto = (solido + degradado).min(rect.height() * 0.6);
    if alto <= 1.0 {
        return;
    }
    let fondo = theme::tarjeta();
    let arriba = egui::Color32::from_rgba_unmultiplied(fondo.r(), fondo.g(), fondo.b(), 0);
    let x0 = rect.left();
    let x1 = (rect.right() - margen_derecho).max(x0);
    let y_solido = rect.bottom() - solido;
    let y0 = (y_solido - degradado).max(rect.top());
    let y2 = rect.bottom();

    let mut malla = egui::Mesh::default();
    malla.colored_vertex(egui::pos2(x0, y0), arriba);
    malla.colored_vertex(egui::pos2(x1, y0), arriba);
    malla.colored_vertex(egui::pos2(x0, y_solido), fondo);
    malla.colored_vertex(egui::pos2(x1, y_solido), fondo);
    malla.colored_vertex(egui::pos2(x0, y2), fondo);
    malla.colored_vertex(egui::pos2(x1, y2), fondo);
    malla.add_triangle(0, 1, 2);
    malla.add_triangle(1, 3, 2);
    malla.add_triangle(2, 3, 4);
    malla.add_triangle(3, 5, 4);
    ui.painter().add(egui::Shape::mesh(malla));
}

fn barra_feedback_venta(
    ui: &mut egui::Ui,
    ancho: f32,
    alto: f32,
    feedback: Option<&FeedbackVenta>,
    rail: bool,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ancho, alto), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme::lima_tenue());
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top()),
            egui::vec2(rect.width(), 1.0),
        ),
        egui::CornerRadius::ZERO,
        theme::tenir(theme::acento(), 0.18),
    );
    let mut fui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(if rail { 10.0 } else { 14.0 }, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    fui.spacing_mut().item_spacing.x = 0.0;

    let (icono, _) = fui.allocate_exact_size(
        egui::vec2(20.0, 20.0),
        egui::Sense::hover(),
    );
    fui.painter().circle_filled(
        icono.center(),
        10.0,
        theme::tenir(theme::acento(), 0.15),
    );
    Icono::Paloma.dibujar(
        fui.painter(),
        icono.center(),
        12.0,
        theme::acento(),
    );
    fui.add_space(7.0);

    let mut deshacer = false;
    let (titulo, detalle) = match feedback {
        Some(f) => (f.titulo.clone(), f.detalle.clone()),
        None => (
            "Listo para vender".to_string(),
            "Escanea o busca un producto para empezar.".to_string(),
        ),
    };
    fui.allocate_ui_with_layout(
        egui::vec2((fui.available_width() - 86.0).max(100.0), alto),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.add_space(if rail { 4.0 } else { 5.0 });
            ui.label(
                egui::RichText::new(cortar(&titulo, if rail { 34 } else { 48 }))
                    .size(if rail { 9.0 } else { 10.0 })
                    .strong()
                    .color(theme::texto()),
            );
            if !rail {
                ui.label(
                    egui::RichText::new(cortar(&detalle, 62))
                        .size(8.5)
                        .color(theme::texto_suave()),
                );
            }
        },
    );
    fui.with_layout(
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            if feedback.and_then(|f| f.antes.as_ref()).is_some() {
                let resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new("Deshacer")
                            .size(9.0)
                            .strong()
                            .color(theme::acento()),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(0.0, 22.0)),
                );
                deshacer = resp.clicked();
            } else if !rail {
                ui.label(
                    egui::RichText::new("Última acción")
                        .size(8.5)
                        .color(theme::texto_suave()),
                );
            }
        },
    );
    deshacer
}

fn pie_tabla_venta(ui: &mut egui::Ui, ancho: f32, alto: f32, mostrados: usize, total: usize) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ancho, alto), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme::superficie_suave());
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top()),
            egui::vec2(rect.width(), 1.0),
        ),
        egui::CornerRadius::ZERO,
        theme::borde(),
    );
    let mut fui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(16.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    fui.spacing_mut().item_spacing.x = 0.0;
    fui.label(
        egui::RichText::new("F2 buscar   ·   Flechas elegir   ·   Enter agregar   ·   F4 cobrar")
            .size(8.2)
            .color(theme::texto_suave()),
    );
    fui.with_layout(
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(
                egui::RichText::new(format!("Mostrando {mostrados} de {total}"))
                    .size(8.2)
                    .color(theme::texto_suave()),
            );
        },
    );
}

fn badge_cantidad(fui: &mut egui::Ui, qty: &str, rail: bool) -> bool {
    let lado = if rail { 24.0 } else { 26.0 };
    let (rect, respuesta) =
        fui.allocate_exact_size(egui::vec2(lado, lado), egui::Sense::click());
    fui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(8),
        theme::acento_tenue(),
    );
    fui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        qty,
        egui::FontId::proportional(if rail { 10.0 } else { 11.0 }),
        theme::acento(),
    );
    respuesta.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            fui.is_enabled(),
            format!("Cantidad {qty}; escribir otra"),
        )
    });
    respuesta.on_hover_text("Escribir la cantidad").clicked()
}

fn icono_linea(
    ui: &mut egui::Ui,
    icono: Icono,
    color: egui::Color32,
    pista: &str,
) -> bool {
    let (rect, respuesta) =
        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    if respuesta.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(5),
            theme::tarjeta_alta(),
        );
    }
    icono.dibujar(ui.painter(), rect.center(), 11.0, color);
    respuesta.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), pista)
    });
    respuesta.on_hover_text(pista).clicked()
}

fn boton_descuento_ticket(ui: &mut egui::Ui, rail: bool, abierto: bool) -> bool {
    let (rect, respuesta) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), if rail { 24.0 } else { 27.0 }),
        egui::Sense::click(),
    );
    if respuesta.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            theme::superficie_suave(),
        );
    }
    let mut fui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(7.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    fui.spacing_mut().item_spacing.x = 0.0;
    let (r, _) = fui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
    Icono::Etiqueta.dibujar(
        fui.painter(),
        r.center(),
        13.0,
        theme::acento(),
    );
    fui.add_space(7.0);
    fui.label(
        egui::RichText::new("Descuento o fiado")
            .size(if rail { 9.5 } else { 10.0 })
            .color(theme::texto_suave()),
    );
    fui.with_layout(
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(
                egui::RichText::new(if abierto { "Ocultar" } else { "Agregar" })
                    .size(9.0)
                    .strong()
                    .color(theme::acento()),
            );
        },
    );
    respuesta.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            "Descuento o fiado",
        )
    });
    respuesta.clicked()
}

fn fila_ticket(
    ui: &mut egui::Ui,
    etiqueta: &str,
    valor: &str,
    color: Option<egui::Color32>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(etiqueta)
                .size(10.0)
                .color(theme::texto_suave()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(valor)
                    .size(10.5)
                    .strong()
                    .color(color.unwrap_or(theme::texto())),
            );
        });
    });
}

fn editor_cantidad_ticket(app: &mut PosApp, ui: &mut egui::Ui, i: usize, rail: bool) {
    let mut aplicar = false;
    let mut cancelar = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            egui::RichText::new("Cantidad")
                .size(if rail { 9.0 } else { 9.5 })
                .color(theme::texto_suave()),
        );
        ui.add_sized(
            [62.0, 24.0],
            egui::TextEdit::singleline(&mut app.cantidad_linea_txt)
                .hint_text("12")
                .font(egui::FontId::proportional(10.0)),
        );
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("Aplicar")
                        .size(9.0)
                        .strong()
                        .color(theme::acento()),
                )
                .fill(theme::acento_tenue())
                .stroke(egui::Stroke::NONE)
                .corner_radius(egui::CornerRadius::same(6))
                .min_size(egui::vec2(0.0, 24.0)),
            )
            .clicked()
        {
            aplicar = true;
        }
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("Cancelar")
                        .size(9.0)
                        .color(theme::texto_suave()),
                )
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(1.0_f32, theme::borde()))
                .corner_radius(egui::CornerRadius::same(6))
                .min_size(egui::vec2(0.0, 24.0)),
            )
            .clicked()
        {
            cancelar = true;
        }
    });
    if aplicar {
        app.aplicar_cantidad_linea(i);
    }
    if cancelar {
        app.cantidad_linea = None;
    }
}

fn editor_precio_ticket(app: &mut PosApp, ui: &mut egui::Ui, i: usize, rail: bool) {
    let mut aplicar = false;
    let mut cancelar = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            egui::RichText::new("Precio")
                .size(if rail { 9.0 } else { 9.5 })
                .color(theme::texto_suave()),
        );
        ui.add_sized(
            [72.0, 24.0],
            egui::TextEdit::singleline(&mut app.precio_linea_txt)
                .hint_text("0.00")
                .font(egui::FontId::proportional(10.0)),
        );
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("Aplicar")
                        .size(9.0)
                        .strong()
                        .color(theme::acento()),
                )
                .fill(theme::acento_tenue())
                .stroke(egui::Stroke::NONE)
                .corner_radius(egui::CornerRadius::same(6))
                .min_size(egui::vec2(0.0, 24.0)),
            )
            .clicked()
        {
            aplicar = true;
        }
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("Cancelar")
                        .size(9.0)
                        .color(theme::texto_suave()),
                )
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(1.0_f32, theme::borde()))
                .corner_radius(egui::CornerRadius::same(6))
                .min_size(egui::vec2(0.0, 24.0)),
            )
            .clicked()
        {
            cancelar = true;
        }
    });
    if aplicar {
        let antes = app.venta.clone();
        let nombre = app
            .venta
            .lines
            .get(i)
            .map(|l| l.description.clone())
            .unwrap_or_default();
        if let Some(p) = Money::parse(&app.precio_linea_txt) {
            if p.cents() >= 0 {
                if let Some(linea) = app.venta.lines.get_mut(i) {
                    linea.unit_price = p;
                }
                app.mensaje = format!("Precio cambiado a {p}. Queda anotado al cobrar.");
                app.feedback_venta = Some(FeedbackVenta {
                    titulo: format!("Precio de {nombre} actualizado"),
                    detalle: format!("Ahora cuesta {p}."),
                    sku: None,
                    antes: Some(antes),
                });
            } else {
                app.mensaje = "Ese precio no se entiende".to_string();
            }
        } else {
            app.mensaje = "Ese precio no se entiende".to_string();
        }
        app.precio_linea = None;
    }
    if cancelar {
        app.precio_linea = None;
    }
}


fn pestana(
    ui: &mut egui::Ui,
    activa: bool,
    nombre: &str,
    atajo: &str,
    con_atajo: bool,
    icono: Icono,
) -> bool {
    let etiqueta = if con_atajo && !atajo.is_empty() {
        format!("{nombre}  {atajo}")
    } else {
        nombre.to_string()
    };
    let tinta = if activa {
        theme::acento()
    } else {
        theme::texto()
    };
    let galley_nom =
        ui.fonts(|f| f.layout_no_wrap(nombre.to_string(), egui::FontId::proportional(15.0), tinta));
    let mostrar_atajo = con_atajo && !atajo.is_empty();
    let galley_at = if mostrar_atajo {
        Some(ui.fonts(|f| {
            f.layout_no_wrap(
                atajo.to_string(),
                egui::FontId::proportional(11.0),
                theme::texto_suave(),
            )
        }))
    } else {
        None
    };

    let w_atajo = galley_at.as_ref().map(|g| 5.0 + g.size().x).unwrap_or(0.0);
    let alto = 38.0;
    let ancho = 10.0 + 16.0 + 6.0 + galley_nom.size().x + w_atajo + 12.0;
    let (rect, respuesta) = ui.allocate_exact_size(egui::vec2(ancho, alto), egui::Sense::click());
    respuesta.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), etiqueta.clone())
    });
    let pintor = ui.painter();

    if activa {
        // Raya bajo el texto, no pegada al borde de la barra: si se pinta
        // en el filo, se confunde con cualquier línea de abajo y parece
        // que ninguna pestaña está activa.
        pintor.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + 10.0, rect.bottom() - 5.0),
                egui::pos2(rect.right() - 10.0, rect.bottom() - 2.0),
            ),
            egui::CornerRadius::same(2),
            theme::acento(),
        );
    } else if respuesta.hovered() {
        pintor.rect_filled(
            rect.shrink2(egui::vec2(2.0, 4.0)),
            egui::CornerRadius::same(8),
            theme::tenir(theme::acento(), 0.08),
        );
    }

    let y_centro = rect.center().y - 1.5;
    icono.dibujar(
        pintor,
        egui::pos2(rect.left() + 10.0 + 8.0, y_centro),
        16.0,
        tinta,
    );
    let x_nom = rect.left() + 10.0 + 16.0 + 6.0;
    let w_nom = galley_nom.size().x;
    let h_nom = galley_nom.size().y;
    pintor.galley(egui::pos2(x_nom, y_centro - h_nom * 0.5), galley_nom, tinta);
    if let Some(g) = galley_at {
        let h_at = g.size().y;
        pintor.galley(
            egui::pos2(x_nom + w_nom + 5.0, y_centro - h_at * 0.5 + 1.0),
            g,
            theme::texto_suave(),
        );
    }

    if !con_atajo && !atajo.is_empty() {
        respuesta.clone().on_hover_text(format!("Atajo: {atajo}"));
    }
    respuesta.clicked()
}

/// Un palito entre el reloj, la caja y Salir: sin él el rincón derecho
/// es tres palabras sueltas flotando.
fn raya_vertical(ui: &mut egui::Ui, alto: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, alto), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme::borde());
}

/// Una barrita de comparación para las tablas. `fraccion` va de 0 a 1 y el
/// ancho es el de la barra llena; siempre se dibuja dentro del espacio que
/// se le dio, así que no empuja nada.
fn barra(ui: &mut egui::Ui, ancho: f32, fraccion: f32, color: egui::Color32) {
    let fraccion = fraccion.clamp(0.0, 1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ancho, 12.0), egui::Sense::hover());
    let esquinas = egui::CornerRadius::same(6);
    ui.painter()
        .rect_filled(rect, esquinas, color.gamma_multiply(0.20));
    if fraccion > 0.0 {
        let lleno = egui::Rect::from_min_size(
            rect.min,
            egui::vec2((rect.width() * fraccion).max(4.0), rect.height()),
        );
        ui.painter().rect_filled(lleno, esquinas, color);
    }
}

/// Un campo de la configuración: la etiqueta chiquita y gris arriba, el campo
/// debajo ocupando todo el ancho que le den.
///
/// En fila (etiqueta a la izquierda, campo a la derecha) los campos quedan
/// disparejos porque las etiquetas no miden lo mismo; uno debajo del otro se
/// alinean solos y la pantalla se lee en columna, como un formulario de verdad.
fn campo_grande(ui: &mut egui::Ui, etiqueta: &str, valor: &mut String, pista: &str) {
    ui.label(
        egui::RichText::new(etiqueta)
            .size(12.0)
            .color(theme::texto_suave()),
    );
    ui.add_sized(
        [ui.available_width(), 40.0],
        egui::TextEdit::singleline(valor).hint_text(pista),
    );
    ui.add_space(8.0);
}

/// Una tarjeta con un número grande arriba y su etiqueta abajo. Sirve para
/// los datos de un vistazo (cuántos clientes, cuánto deben) y siempre ocupa
/// el ancho de su columna, así que no se encima con la de al lado.
fn tarjeta_dato(
    ui: &mut egui::Ui,
    etiqueta: &str,
    valor: &str,
    color: egui::Color32,
    icono: Icono,
) {
    let ancho = ui.available_width();
    theme::marco_tarjeta(ui, |ui| {
        ui.horizontal(|ui| {
            // El icono en un círculo del mismo color, rebajado: es lo que
            // hace que el número se entienda antes de leerlo.
            let (r, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(r.center(), 22.0, theme::tenir(color, 0.18));
            icono.dibujar(ui.painter(), r.center(), 22.0, color);
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(
                egui::vec2((ancho - 24.0 - 54.0).max(40.0), 50.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.label(egui::RichText::new(valor).size(32.0).strong().color(color));
                    ui.label(
                        egui::RichText::new(etiqueta)
                            .size(12.5)
                            .color(theme::texto_suave()),
                    );
                },
            );
        });
    });
}

/// La tarjeta de una persona en el inicio de sesión: iniciales, nombre y
/// el rol en su pastilla. Devuelve `true` si la picaron.
fn tarjeta_usuario(ui: &mut egui::Ui, u: &pos_api::UsuarioDto) -> bool {
    let tam = egui::vec2((ui.available_width() - 8.0).max(200.0), 82.0);
    let (rect, respuesta) = ui.allocate_exact_size(tam, egui::Sense::click());
    let pintor = ui.painter();
    let radio = egui::CornerRadius::same(12);
    pintor.rect_filled(
        rect,
        radio,
        if respuesta.hovered() {
            theme::tarjeta_alta()
        } else {
            theme::tarjeta()
        },
    );
    pintor.rect_stroke(
        rect,
        radio,
        egui::Stroke::new(
            if respuesta.hovered() {
                1.6_f32
            } else {
                1.0_f32
            },
            if respuesta.hovered() {
                theme::acento()
            } else {
                theme::borde()
            },
        ),
        egui::StrokeKind::Inside,
    );

    let avatar = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 14.0, rect.center().y - 24.0),
        egui::vec2(48.0, 48.0),
    );
    theme::pintar_iniciales(pintor, avatar, &u.display_name, theme::acento());

    let area = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 74.0, rect.top() + 14.0),
        egui::vec2(rect.width() - 88.0, rect.height() - 28.0),
    );
    pintor.text(
        area.left_top(),
        egui::Align2::LEFT_TOP,
        cortar(&u.display_name, 22),
        egui::FontId::proportional(16.0),
        theme::texto(),
    );
    // El rol, en una pastilla: se distingue de un golpe sin leer de más.
    let (texto_rol, color_rol) = match u.role.as_str() {
        "Admin" => ("Administrador", theme::peligro()),
        "Supervisor" => ("Supervisor", theme::advertencia()),
        _ => ("Cajero", theme::exito()),
    };
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            texto_rol.to_string(),
            egui::FontId::proportional(12.0),
            color_rol,
        )
    });
    let caja = egui::Rect::from_min_size(
        area.left_bottom() - egui::vec2(0.0, galley.size().y + 7.0),
        galley.size() + egui::vec2(16.0, 8.0),
    );
    pintor.rect_filled(
        caja,
        egui::CornerRadius::same(8),
        theme::tenir(color_rol, 0.16),
    );
    pintor.galley(caja.min + egui::vec2(8.0, 4.0), galley, color_rol);
    respuesta.clicked()
}

/// Un campo de texto con su etiqueta encima. La etiqueta va chiquita y
/// arriba para que se vea qué es cada cosa sin tener que adivinar, y el
/// cuadro nunca se estira más del ancho que se le pidió.
fn campo(ui: &mut egui::Ui, etiqueta: &str, ancho: f32, valor: &mut String, pista: &str) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(etiqueta)
                .size(12.0)
                .color(theme::texto_suave()),
        );
        let editor = if pista.is_empty() {
            egui::TextEdit::singleline(valor)
        } else {
            egui::TextEdit::singleline(valor).hint_text(pista)
        };
        // Cero = llena la columna: así el nombre no se queda en 200 px
        // cuando la pantalla da 600.
        let ancho = if ancho <= 0.0 {
            ui.available_width()
        } else {
            ancho
        };
        ui.add_sized([ancho, 40.0], editor);
    });
}

fn combo_clave(
    ui: &mut egui::Ui,
    etiqueta: &str,
    id: &str,
    valor: &mut String,
    lista: &[(&str, &str)],
) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(etiqueta)
                .size(12.0)
                .color(theme::texto_suave()),
        );
        let texto = lista
            .iter()
            .find(|(c, _)| *c == valor.as_str())
            .map(|(_, t)| *t)
            .unwrap_or(valor.as_str());
        egui::ComboBox::from_id_salt(id)
            .selected_text(texto.to_string())
            .width(ui.available_width())
            .height(40.0)
            .show_ui(ui, |ui| {
                for (c, t) in lista {
                    if ui.selectable_label(*valor == *c, *t).clicked() {
                        *valor = (*c).to_string();
                    }
                }
            });
    });
}

/// Una baldosa del catálogo en la pantalla de venta: el nombre arriba, el
/// precio grande en el color de acento y, si se está acabando, cuánto queda.
///
/// El botón va **primero y transparente**: es lo que hace clicable toda la
/// baldosa y lo que le pone nombre para quien navega con lector de pantalla.
/// Encima se pinta el contenido, que es lo que se ve.
fn baldosa_producto(
    ui: &mut egui::Ui,
    p: &pos_api::ProductoDto,
    foto: Option<&egui::TextureHandle>,
) -> bool {
    // Llena la columna: si se queda en 180 px fijos, el catálogo se ve
    // como un puñado de islas con huecos a los lados.
    let tam = egui::vec2(ui.available_width(), 122.0);
    let (rect, _) = ui.allocate_exact_size(tam, egui::Sense::hover());
    let respuesta = ui.put(
        rect,
        egui::Button::new(&p.name)
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(14)),
    );

    let pintor = ui.painter().clone();
    let radio = egui::CornerRadius::same(14);
    // Sombra propia: sin ella la baldosa es un recorte plano sobre el fondo.
    pintor.rect_filled(
        rect.translate(egui::vec2(0.0, 3.0)),
        radio,
        egui::Color32::from_black_alpha(28),
    );
    pintor.rect_filled(
        rect,
        radio,
        if respuesta.hovered() {
            theme::tarjeta_alta()
        } else {
            theme::tarjeta()
        },
    );
    // Luz de borde superior: el cartón no brilla, el papel sí.
    pintor.hline(
        (rect.left() + 10.0)..=(rect.right() - 10.0),
        rect.top() + 1.5,
        egui::Stroke::new(1.2_f32, egui::Color32::from_white_alpha(90)),
    );
    let bajo_minimo =
        p.track_inventory && p.min_stock_millis > 0 && p.stock_millis <= p.min_stock_millis;
    let trazo = if respuesta.hovered() {
        egui::Stroke::new(1.6_f32, theme::acento())
    } else if bajo_minimo {
        egui::Stroke::new(1.0_f32, theme::advertencia())
    } else {
        egui::Stroke::new(1.0_f32, theme::borde())
    };
    pintor.rect_stroke(rect, radio, trazo, egui::StrokeKind::Inside);
    // Una raya del color de acento a la izquierda: es lo que hace que una
    // baldosa se distinga de un recuadro de texto.
    let raya = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(0.0, 10.0),
        egui::pos2(rect.left() + 4.0, rect.bottom() - 10.0),
    );
    let color_raya = if p.track_inventory && p.stock_millis <= 0 {
        theme::peligro()
    } else if bajo_minimo || (p.track_inventory && p.stock_millis <= 1_000) {
        theme::advertencia()
    } else {
        theme::acento()
    };
    pintor.rect_filled(raya, egui::CornerRadius::same(2), color_raya);

    let foto_r = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 14.0, rect.center().y - 28.0),
        egui::vec2(56.0, 56.0),
    );
    if let Some(tex) = foto {
        ui.put(
            foto_r,
            egui::Image::new(tex)
                .fit_to_exact_size(egui::vec2(56.0, 56.0))
                .corner_radius(egui::CornerRadius::same(12)),
        );
    } else {
        theme::pintar_foto_producto(&pintor, foto_r, &p.name);
    }

    let area = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 80.0, rect.top() + 12.0),
        egui::vec2(rect.width() - 94.0, rect.height() - 24.0),
    );
    // El nombre en dos renglones: cortar a 24 letras dejaba "Producto número
    // 0 de la…" en todas las baldosas y el catálogo se veía igual.
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = area.width();
    job.wrap.max_rows = 2;
    job.wrap.overflow_character = Some('…');
    job.append(
        &p.name,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(15.0),
            color: theme::texto(),
            ..Default::default()
        },
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    let alto_nombre = galley.size().y;
    pintor.galley(area.left_top(), galley, theme::texto());
    pintor.text(
        egui::pos2(area.left(), area.top() + alto_nombre + 2.0),
        egui::Align2::LEFT_TOP,
        Money::from_cents(p.price_cents).format_mxn(),
        egui::FontId::proportional(24.0),
        theme::acento(),
    );
    // El color dice el estado: rojo = no hay, ámbar = se acaba, gris = hay.
    if p.track_inventory {
        let (texto, color) = if p.stock_millis <= 0 {
            ("agotado".to_string(), theme::peligro())
        } else if bajo_minimo || p.stock_millis <= 1_000 {
            (
                format!("quedan {}", Quantity::from_millis(p.stock_millis).format()),
                theme::advertencia(),
            )
        } else {
            (
                format!("hay {}", Quantity::from_millis(p.stock_millis).format()),
                theme::texto_suave(),
            )
        };
        pintor.text(
            area.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            texto,
            egui::FontId::proportional(11.5),
            color,
        );
    }
    respuesta.clicked()
}

/// Recorta un texto largo para que quepa en la baldosa, sin cortar palabras
/// a la mitad.
fn cortar(texto: &str, max: usize) -> String {
    if texto.chars().count() <= max {
        return texto.to_string();
    }
    let corto: String = texto.chars().take(max).collect();
    match corto.rfind(' ') {
        Some(espacio) if espacio > max / 2 => format!("{}\u{2026}", &corto[..espacio]),
        _ => format!("{corto}\u{2026}"),
    }
}

fn direcciones_lan() -> Vec<String> {
    pos_hw::direcciones_lan()
}

/// Deja primero la dirección que más probablemente ve el celular: una
/// privada (192.168, 10 o 172.16-31), después las públicas y al final las
/// link-local. Así la tarjeta no abre con la IP de una VPN o de VirtualBox.
fn ordenar_ips_lan(ips: &mut [String]) {
    ips.sort_by_key(|ip| prioridad_ip(ip));
}

fn prioridad_ip(ip: &str) -> (u8, u8) {
    let mut partes = ip.split('.');
    let a = partes
        .next()
        .and_then(|p| p.parse::<u8>().ok())
        .unwrap_or(0);
    let b = partes
        .next()
        .and_then(|p| p.parse::<u8>().ok())
        .unwrap_or(0);
    match (a, b) {
        (192, 168) => (0, 0),
        (10, _) => (0, 1),
        (172, 16..=31) => (0, 2),
        (169, 254) => (2, 0),
        _ => (1, 0),
    }
}

fn ip_privada(ip: &str) -> bool {
    prioridad_ip(ip).0 == 0
}

impl PosApp {
    pub fn new(mut api: Box<dyn PosApi>, dry_run: bool) -> Self {
        let usuarios = api.usuarios_login().unwrap_or_default();
        let config = match api.ejecutar(Peticion {
            token: None,
            comando: Comando::LeerConfig,
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Configuracion(c)) => c,
            _ => ConfigDto {
                tienda: "Mi Tienda".into(),
                direccion: String::new(),
                pie: "¡Gracias por su compra!".into(),
                impresora: String::new(),
                ancho_ticket: 48,
                impresora_perfil: pos_hw::Perfil::default().nombre,
                impresora_baudios: 9600,
                abrir_cajon: true,
                respaldo_automatico: true,
                respaldo_carpeta: String::new(),
                respaldo_cada_dias: 1,
                respaldos_a_conservar: 14,
                respaldar_al_cortar: true,
                puerto_lan: 8080,
            },
        };
        PosApp {
            api,
            pantalla: Pantalla::Login,
            token: None,
            usuario: None,
            usuarios,
            form_usu_id: None,
            form_usu_username: String::new(),
            form_usu_nombre: String::new(),
            form_usu_pin: String::new(),
            form_usu_rol: "cajero".into(),
            pin: String::new(),
            pin_error: String::new(),
            pendiente: None,
            venta: Sale::new(""),
            resultados: Vec::new(),
            consulta: String::new(),
            venta_cant: "1".into(),
            recientes: Vec::new(),
            mostrar_alta_producto: false,
            filtro_catalogo: String::new(),
            mostrar_alta_cliente: false,
            filtro_clientes: String::new(),
            foco_busqueda: true,
            mostrar_cobro: false,
            descuento_abierto: false,
            filtro_venta: "todo".into(),
            producto_sel: None,
            feedback_venta: None,
            pagos: vec![FilaPago {
                metodo: PaymentMethod::Efectivo,
                monto: String::new(),
                recibido: String::new(),
            }],
            descuento_linea: String::new(),
            descuento_global: String::new(),
            cliente_id: None,
            cliente_nombre: String::new(),
            clientes: Vec::new(),
            caja: EstadoCajaDto {
                abierta: false,
                sesion_id: None,
                fondo_cents: 0,
                ventas_efectivo_cents: 0,
                entradas_cents: 0,
                retiros_cents: 0,
                gastos_cents: 0,
                esperado_cents: 0,
                tickets: 0,
                vendido_cents: 0,
            },
            abrir_caja_monto: String::new(),
            mostrar_abrir_caja: false,
            cargar_tras_entrar: false,
            mostrar_arqueo: false,
            arqueo: DENOMINACIONES_MXN.iter().map(|(d, _)| (*d, 0i64)).collect(),
            mov_tipo: "retiro".to_string(),
            mov_monto: String::new(),
            mov_motivo: String::new(),
            ultimo_cierre: None,
            form_cliente: NuevoClienteDto::default(),
            form_cliente_limite: String::new(),
            abono_monto: String::new(),
            cliente_abono: None,
            cajeros: Vec::new(),
            desde: String::new(),
            hasta: String::new(),
            folio_buscar: String::new(),
            mensaje: String::new(),
            vista_previa: None,
            fiscal: ConfigFiscalDto::default(),
            facturas: Vec::new(),
            factura_fecha: String::new(),
            factura_periodicidad: "01".to_string(),
            nota_folio: String::new(),
            nota_fecha: String::new(),
            nota_renglones: Vec::new(),
            factura_individual_folio: String::new(),
            factura_individual_fecha: String::new(),
            pendientes_rfc: 0,
            nota_monto: String::new(),
            // La apariencia se lee del disco al arrancar (en `run`), no aquí:
            // `new` se usa en las pruebas y no debe depender de la máquina.
            apariencia: crate::theme::Apariencia::por_defecto(),
            pago_cliente_id: 0,
            pago_saldo_cents: 0,
            pago_monto: String::new(),
            pago_fecha: String::new(),
            pago_forma: "01".to_string(),
            factura_xml: None,
            cancel_factura_id: None,
            cancel_motivo: "02".to_string(),
            cancel_sustitucion: String::new(),
            precio_linea: None,
            precio_linea_txt: String::new(),
            cantidad_linea: None,
            cantidad_linea_txt: String::new(),
            bajo_minimo: Vec::new(),
            ruta_respaldo: String::new(),
            respaldos: EstadoRespaldosDto::default(),
            unidades: Vec::new(),
            bitacora: EventosAuditoriaDto::default(),
            aud_desde: String::new(),
            aud_hasta: String::new(),
            aud_usuario: String::new(),
            aud_accion: String::new(),
            aud_texto: String::new(),
            editando_id: None,
            ruta_csv: String::new(),
            csv_actualizar: false,
            ultimo_importe: None,
            inv_producto: None,
            inv_cantidad: String::new(),
            inv_motivo: String::new(),
            alta_nombre: String::new(),
            alta_precio: String::new(),
            alta_stock: String::new(),
            alta_costo: String::new(),
            alta_codigo: String::new(),
            alta_mensaje: String::new(),
            alta_ok: false,
            foco_alta: false,
            proveedores: Vec::new(),
            form_proveedor_nombre: String::new(),
            form_proveedor_telefono: String::new(),
            pedidos: Vec::new(),
            pedido: None,
            pedido_proveedor: None,
            pedido_codigo: String::new(),
            pedido_qty: "1".into(),
            pedido_costo: String::new(),
            pedido_entregas: Vec::new(),
            conteo: ConteoDto::default(),
            conteo_codigo: String::new(),
            conteo_cantidad: String::new(),
            conteo_categoria: String::new(),
            conteo_filtro: "todos".to_string(),
            mostrar_firma_conteo: false,
            mostrar_mapeo: false,
            analisis: None,
            mapeo: Vec::new(),
            actualizacion: EstadoActualizacionDto::default(),
            ruta_paquete: String::new(),
            paquete: None,
            impresoras: Vec::new(),
            categorias: Vec::new(),
            categoria_nueva: String::new(),
            config,
            dry_run,
            form: FormProducto::default(),
            fotos: std::collections::HashMap::new(),
        }
    }

    // --------------------------------------------------------- auxiliares --

    fn token(&self) -> Option<String> {
        self.token.clone()
    }

    fn ejecutar(&mut self, comando: Comando) -> Cuerpo {
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando,
        }) {
            pos_api::Respuesta::Ok(c) => c,
            pos_api::Respuesta::Error(e) => {
                self.mensaje = e.clone();
                Cuerpo::Vacio
            }
        }
    }

    /// Carga el catálogo completo. La usan las pruebas de Unix (arnés
    /// kittest); la interfaz lo hace buscando conforme se escribe.
    #[cfg(all(test, unix))]
    fn cargar_catalogo(&mut self) {
        if let pos_api::Respuesta::Ok(pos_api::Cuerpo::Productos(v)) = self.api.ejecutar(Peticion {
            token: self.token.clone(),
            comando: Comando::BuscarProductos {
                termino: String::new(),
                limite: 1_000_000,
            },
        }) {
            self.resultados = v;
        }
    }

    /// Usa esta apariencia en vez de la de siempre. La llama `run` al abrir
    /// la ventana, con lo que la persona dejó guardado.
    pub fn con_apariencia(mut self, apariencia: crate::theme::Apariencia) -> Self {
        self.apariencia = apariencia;
        self
    }

    fn refrescar_caja(&mut self) {
        if let Cuerpo::Caja(c) = self.ejecutar(Comando::EstadoCaja) {
            self.caja = c;
        }
    }

    fn refrescar_clientes(&mut self) {
        if let Cuerpo::Clientes(c) = self.ejecutar(Comando::Clientes {
            termino: String::new(),
        }) {
            self.clientes = c;
        }
    }

    fn asegurar_foto(&mut self, ctx: &egui::Context, sku: &str) -> Option<egui::TextureHandle> {
        // En Windows glow/WGL se ha llevado el proceso al subir la primera
        // textura JPEG (justo al pasar del login a la venta). El empaque
        // pintado cubre el mismo hueco; en Linux sí se cargan.
        if cfg!(windows) {
            let _ = (ctx, sku);
            return None;
        }
        if let Some(t) = self.fotos.get(sku) {
            return Some(t.clone());
        }
        let bytes = crate::fotos::jpeg(sku)?;
        let img = image::load_from_memory(bytes).ok()?.into_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        if size[0] == 0 || size[1] == 0 {
            return None;
        }
        if size[0].saturating_mul(size[1]).saturating_mul(4) != img.len() {
            return None;
        }
        let color = egui::ColorImage::from_rgba_unmultiplied(size, &img);
        let tex = ctx.load_texture(format!("sku-{sku}"), color, egui::TextureOptions::LINEAR);
        self.fotos.insert(sku.to_string(), tex.clone());
        Some(tex)
    }

    fn buscar(&mut self, termino: &str) {
        if let Some(token) = self.token.clone() {
            if let Ok(lista) = self.api.buscar(&token, termino, 48) {
                self.resultados = lista;
            }
        }
    }

    /// Lo que dice el recuadro de cantidad. Vacío o cero = una pieza.
    fn cantidad_a_agregar(&self) -> Quantity {
        let q = Quantity::parse(self.venta_cant.trim()).unwrap_or(Quantity::ONE);
        if q.millis() <= 0 {
            Quantity::ONE
        } else {
            q
        }
    }

    fn agregar_producto(&mut self, p: &ProductoDto) {
        let qty = self.cantidad_a_agregar();
        let antes = self.venta.clone();
        if p.track_inventory {
            let en_ticket: i64 = self
                .venta
                .lines
                .iter()
                .filter(|l| l.sku == p.sku)
                .map(|l| l.qty.millis())
                .sum();
            if en_ticket + qty.millis() > p.stock_millis {
                self.mensaje = format!(
                    "No hay más '{}': hay {}",
                    p.name,
                    Quantity::from_millis(p.stock_millis).format()
                );
                return;
            }
        }
        let linea = SaleLine {
            product_id: Some(p.id),
            sku: p.sku.clone(),
            description: p.name.clone(),
            qty,
            unit_price: Money::from_cents(p.price_cents),
            discount: Money::ZERO,
            tax: TaxRate::from_bp(p.tax_bp),
            unit_cost: Money::from_cents(p.cost_cents),
        };
        if self.venta.add_line(linea).is_ok() {
            self.mensaje.clear();
            self.venta_cant = "1".into();
            self.foco_busqueda = true;
            self.producto_sel = Some(p.sku.clone());
            self.recientes.retain(|(s, _)| s != &p.sku);
            self.recientes.insert(0, (p.sku.clone(), p.name.clone()));
            self.recientes.truncate(8);
            let en_ticket: i64 = self
                .venta
                .lines
                .iter()
                .filter(|l| l.sku == p.sku)
                .map(|l| l.qty.millis())
                .sum();
            let piezas = Quantity::from_millis(en_ticket).format();
            let importe: i64 = self
                .venta
                .lines
                .iter()
                .filter(|l| l.sku == p.sku)
                .map(|l| l.gross().cents())
                .sum();
            self.feedback_venta = Some(FeedbackVenta {
                titulo: format!("{} agregado", p.name),
                detalle: format!(
                    "{piezas} en el ticket · {}",
                    Money::from_cents(importe).format_mxn()
                ),
                sku: Some(p.sku.clone()),
                antes: Some(antes),
            });
        }
    }

    /// Guarda un punto de retorno y el texto de la barra de feedback.
    fn registrar_feedback(&mut self, titulo: impl Into<String>, detalle: impl Into<String>) {
        self.feedback_venta = Some(FeedbackVenta {
            titulo: titulo.into(),
            detalle: detalle.into(),
            sku: None,
            antes: Some(self.venta.clone()),
        });
    }

    fn deshacer_feedback(&mut self) {
        let Some(f) = self.feedback_venta.clone() else {
            return;
        };
        if let Some(antes) = f.antes {
            self.venta = antes;
            self.mensaje = "Cambio deshecho.".into();
            self.feedback_venta = Some(FeedbackVenta {
                titulo: "Cambio deshecho".into(),
                detalle: "El ticket volvió a como estaba.".into(),
                sku: None,
                antes: None,
            });
        }
    }

    // ------------------------------------------------------------ acciones --

    fn entrar(&mut self) {
        let Some(pendiente) = self.pendiente.clone() else {
            return;
        };
        let pin = self.pin.clone();
        match self.api.login(&pendiente.username, &pin) {
            Ok(sesion) => {
                self.token = Some(sesion.token);
                self.usuario = Some(sesion.usuario);
                self.pendiente = None;
                self.pin.clear();
                self.pin_error.clear();
                self.pantalla = Pantalla::Venta;
                self.foco_busqueda = true;
                self.mensaje.clear();
                // El catálogo, la caja y la ventana de fondo se cargan en
                // el *siguiente* frame: el clic del PIN todavía está dentro
                // del CentralPanel del login, y mezclar eso con SidePanel +
                // 48 baldosas + JPEG tumba Windows.
                self.cargar_tras_entrar = true;
            }
            Err(e) => {
                self.pin_error = e;
                self.pin.clear();
            }
        }
    }

    fn salir(&mut self) {
        self.ejecutar(Comando::Salir);
        self.token = None;
        self.usuario = None;
        self.pantalla = Pantalla::Login;
        self.venta = Sale::new("");
        self.resultados.clear();
        self.mensaje.clear();
    }

    fn aplicar_cantidad_linea(&mut self, i: usize) {
        if i >= self.venta.lines.len() {
            self.cantidad_linea = None;
            return;
        }
        let qty = Quantity::parse(self.cantidad_linea_txt.trim()).unwrap_or(Quantity::ZERO);
        if qty.millis() < 0 {
            self.mensaje = "Esa cantidad no se entiende".into();
            return;
        }
        let sku = self.venta.lines[i].sku.clone();
        let nombre = self.venta.lines[i].description.clone();
        if let Some(hay) = self
            .resultados
            .iter()
            .find(|p| p.sku == sku)
            .filter(|p| p.track_inventory)
            .map(|p| p.stock_millis)
        {
            if qty.millis() > hay {
                self.mensaje = format!(
                    "No hay más '{nombre}': hay {}",
                    Quantity::from_millis(hay).format()
                );
                return;
            }
        }
        self.venta.set_qty(i, qty);
        self.cantidad_linea = None;
        self.cantidad_linea_txt.clear();
        if qty.millis() == 0 {
            self.mensaje = format!("Se quitó {nombre} del ticket");
        }
    }

    fn aplicar_descuento_linea(&mut self) {
        let Some(monto) = Money::parse(&self.descuento_linea) else {
            self.mensaje = "Monto de descuento inválido".into();
            return;
        };
        let n = self.venta.lines.len();
        if n == 0 {
            return;
        }
        let i = n - 1;
        let bruto = Money::from_cents(self.venta.lines[i].unit_price.cents())
            .mul_qty(self.venta.lines[i].qty);
        if monto > bruto {
            self.mensaje = "El descuento es mayor que el importe de la línea".into();
            return;
        }
        self.venta.lines[i].discount = monto;
        self.descuento_linea.clear();
        self.mensaje = format!("Descuento de {monto} aplicado. Queda anotado al cobrar.");
    }

    /// Prepara la ventana de cobro: un pago en efectivo por el total.
    fn abrir_cobro(&mut self) {
        let total = self.venta.total();
        self.pagos = vec![FilaPago {
            metodo: PaymentMethod::Efectivo,
            monto: format!("{:.2}", total.cents() as f64 / 100.0),
            recibido: String::new(),
        }];
    }

    /// Cuánto se ha cubierto con los pagos capturados.
    fn pagado(&self) -> Money {
        self.pagos
            .iter()
            .map(|p| Money::parse(&p.monto).unwrap_or(Money::ZERO))
            .fold(Money::ZERO, |a, b| a + b)
    }

    /// El cambio es la suma de lo que se recibió de más **en efectivo**.
    fn cambio(&self) -> Money {
        self.pagos
            .iter()
            .filter(|p| p.metodo == PaymentMethod::Efectivo)
            .map(|p| {
                let monto = Money::parse(&p.monto).unwrap_or(Money::ZERO);
                let recibido = Money::parse(&p.recibido).unwrap_or(monto);
                (recibido - monto).max(Money::ZERO)
            })
            .fold(Money::ZERO, |a, b| a + b)
    }

    fn construir_venta(&self, total: Money) -> VentaDto {
        // Si no se capturó monto en el primer pago, se asume el total.
        let pagos: Vec<PagoDto> = if self.pagos.is_empty() {
            vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: total.cents(),
                recibido_cents: total.cents(),
            }]
        } else {
            self.pagos
                .iter()
                .map(|p| {
                    let monto = Money::parse(&p.monto).unwrap_or(Money::ZERO);
                    PagoDto {
                        metodo: match p.metodo {
                            PaymentMethod::Efectivo => "efectivo".into(),
                            PaymentMethod::Tarjeta => "tarjeta".into(),
                            PaymentMethod::Transferencia => "transferencia".into(),
                            PaymentMethod::Vales => "vales".into(),
                            PaymentMethod::Credito => "credito".into(),
                        },
                        monto_cents: monto.cents(),
                        recibido_cents: if p.metodo == PaymentMethod::Efectivo {
                            Money::parse(&p.recibido).unwrap_or(monto).cents()
                        } else {
                            0
                        },
                    }
                })
                .collect()
        };

        VentaDto {
            lineas: self
                .venta
                .lines
                .iter()
                .map(|l| LineaDto {
                    sku: l.sku.clone(),
                    descripcion: l.description.clone(),
                    qty_millis: l.qty.millis(),
                    precio_cents: l.unit_price.cents(),
                    descuento_cents: l.discount.cents(),
                })
                .collect(),
            descuento_global_cents: Money::parse(&self.descuento_global)
                .unwrap_or(Money::ZERO)
                .cents(),
            pagos,
            cliente_id: self.cliente_id,
            nota: String::new(),
            supervisor: None,
        }
    }

    fn cobrar(&mut self) {
        let total = self.venta.total();
        let pagado = self.pagado();
        if pagado < total {
            self.mensaje = format!("Falta por cubrir {}", total - pagado);
            return;
        }
        if pagado > total {
            self.mensaje = format!(
                "Te sobran {}: revisa los montos de los pagos",
                pagado - total
            );
            return;
        }
        for p in &self.pagos {
            if p.metodo == PaymentMethod::Efectivo {
                let monto = Money::parse(&p.monto).unwrap_or(Money::ZERO);
                let recibido = Money::parse(&p.recibido).unwrap_or(monto);
                if recibido < monto {
                    self.mensaje = "El cliente entregó menos del monto en efectivo".into();
                    return;
                }
            }
        }
        if self
            .pagos
            .iter()
            .any(|p| p.metodo == PaymentMethod::Credito)
            && self.cliente_id.is_none()
        {
            self.mensaje = "Elige un cliente para vender a crédito".into();
            return;
        }

        let dto = self.construir_venta(total);
        let Some(token) = self.token.clone() else {
            return;
        };

        match self.api.cobrar(&token, dto) {
            Ok(guardada) => {
                let datos = self.datos_ticket(&guardada.folio);
                // El cajón abre sólo si la tienda lo pidió y pagaron en
                // efectivo: en una boutique nadie quiere el cajón abierto
                // cada vez que pasan la tarjeta.
                let abrir_cajon = !self.dry_run
                    && self.config.abrir_cajon
                    && self
                        .pagos
                        .iter()
                        .any(|p| p.metodo == PaymentMethod::Efectivo);
                let bytes = datos.to_escpos_con(&self.perfil(), abrir_cajon);
                self.imprimir(bytes);
                self.mensaje = format!(
                    "✓ Venta {} cobrada · Cambio {}",
                    guardada.folio,
                    Money::from_cents(guardada.cambio_cents)
                );
                self.venta = Sale::new("");
                self.pagos = vec![FilaPago {
                    metodo: PaymentMethod::Efectivo,
                    monto: String::new(),
                    recibido: String::new(),
                }];
                self.descuento_global.clear();
                self.cliente_id = None;
                self.cliente_nombre.clear();
                self.mostrar_cobro = false;
                self.venta_cant = "1".into();
                self.foco_busqueda = true;
                self.buscar("");
                self.refrescar_caja();
                self.refrescar_alertas();
            }
            Err(e) => self.mensaje = e,
        }
    }

    /// Alerta de productos en mínimo: se refresca al vender y al entrar.
    fn refrescar_alertas(&mut self) {
        if let Cuerpo::Productos(p) = self.ejecutar(Comando::ProductosBajoMinimo) {
            self.bajo_minimo = p;
        }
    }

    fn datos_ticket(&self, folio: &str) -> ReceiptData {
        ReceiptData {
            store_name: self.config.tienda.clone(),
            address: self.config.direccion.clone(),
            footer: self.config.pie.clone(),
            folio: folio.to_string(),
            fecha: ahora_cdmx(),
            cajero: self
                .usuario
                .as_ref()
                .map(|u| u.display_name.clone())
                .unwrap_or_default(),
            items: self
                .venta
                .lines
                .iter()
                .map(|l| ReceiptItem {
                    qty: l.qty.format(),
                    description: l.description.clone(),
                    importe: l.gross(),
                })
                .collect(),
            subtotal: self.venta.subtotal(),
            descuento: self.venta.discount_total(),
            impuesto: self.venta.tax_total(),
            total: self.venta.total(),
            pagos: self
                .pagos
                .iter()
                .map(|p| {
                    (
                        p.metodo.label().to_string(),
                        Money::parse(&p.monto).unwrap_or(Money::ZERO),
                    )
                })
                .collect(),
            cambio: self.cambio(),
            qr: Some(format!("https://mitienda.mx/t/{folio}")),
        }
    }

    /// Reconstruye el ticket de una venta ya guardada, para reimprimirlo.
    fn datos_ticket_guardado(&self, t: &TicketDto) -> ReceiptData {
        ReceiptData {
            store_name: self.config.tienda.clone(),
            address: self.config.direccion.clone(),
            footer: self.config.pie.clone(),
            folio: t.folio.clone(),
            fecha: t.fecha.clone(),
            cajero: t.cajero.clone(),
            items: t
                .lineas
                .iter()
                .map(|l| ReceiptItem {
                    qty: Quantity::from_millis(l.qty_millis).format(),
                    description: l.descripcion.clone(),
                    importe: Money::from_cents(l.importe_cents),
                })
                .collect(),
            subtotal: Money::from_cents(t.subtotal_cents),
            descuento: Money::from_cents(t.descuento_cents),
            impuesto: Money::from_cents(t.impuesto_cents),
            total: Money::from_cents(t.total_cents),
            pagos: t
                .pagos
                .iter()
                .map(|p| {
                    (
                        etiqueta_metodo(&p.metodo).to_string(),
                        Money::from_cents(p.monto_cents),
                    )
                })
                .collect(),
            cambio: Money::from_cents(t.cambio_cents),
            qr: Some(format!("https://mitienda.mx/t/{}", t.folio)),
        }
    }

    /// Perfil de la impresora según lo que configuró el encargado.
    fn perfil(&self) -> pos_hw::Perfil {
        pos_hw::Perfil::por_nombre(&self.config.impresora_perfil)
            .unwrap_or_default()
            .con_columnas(self.config.ancho_ticket as usize)
            .con_baudios(self.config.impresora_baudios.max(0) as u32)
    }

    /// Por dónde sale el ticket: USB, serie o red, según la ruta escrita.
    fn transporte(&self) -> pos_hw::Transporte {
        pos_hw::Transporte::desde_texto(
            &self.config.impresora,
            self.config.impresora_baudios.max(0) as u32,
        )
    }

    fn imprimir(&mut self, bytes: Vec<u8>) {
        // La vista previa se decodifica con la página de códigos del perfil:
        // si se leyera como UTF-8, los acentos saldrían como basura justo en
        // la pantalla donde el cajero revisa el ticket.
        let limpio = pos_hw::strip_commands(&bytes);
        let vista = pos_hw::decodificar(&limpio, self.perfil().pagina);
        self.vista_previa = Some(vista);

        if self.dry_run || self.config.impresora.trim().is_empty() {
            if self.config.impresora.trim().is_empty() {
                self.mensaje = "Ticket en vista previa (no hay impresora configurada)".into();
            }
            return;
        }
        let transporte = self.transporte();
        match transporte.imprimir(&self.perfil(), &bytes) {
            Ok(()) => {}
            Err(e) => {
                // El mensaje ya viene escrito para el encargado ("¿Está
                // encendida?"), así que se muestra tal cual.
                self.mensaje = format!("No se pudo imprimir: {e}");
            }
        }
    }

    // ------------------------------------------------------------ teclado --

    fn tick_teclado(&mut self, ctx: &egui::Context) {
        let (f2, f3, f4, f5, f6, f7, f8, f9, f10, esc, enter, supr, plus, minus) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F2),
                i.key_pressed(egui::Key::F3),
                i.key_pressed(egui::Key::F4),
                i.key_pressed(egui::Key::F5),
                i.key_pressed(egui::Key::F6),
                i.key_pressed(egui::Key::F7),
                i.key_pressed(egui::Key::F8),
                i.key_pressed(egui::Key::F9),
                i.key_pressed(egui::Key::F10),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals),
                i.key_pressed(egui::Key::Minus),
            )
        });

        if self.pantalla == Pantalla::Login {
            if enter && !self.pin.is_empty() {
                self.entrar();
            }
            return;
        }

        if esc {
            if self.mostrar_arqueo {
                self.mostrar_arqueo = false;
            } else if self.mostrar_abrir_caja {
                self.mostrar_abrir_caja = false;
            } else if self.mostrar_cobro {
                self.mostrar_cobro = false;
            } else if self.pantalla != Pantalla::Venta {
                self.pantalla = Pantalla::Venta;
            }
        }
        // Los atajos pasan por el mismo lugar que las pestañas: si se
        // agrega una pantalla, se navega igual con el teclado y con el mouse.
        if f2 {
            self.ir_a(Pantalla::Venta);
        }
        if f3 {
            self.ir_a(Pantalla::Catalogo);
        }
        if f5 {
            self.ir_a(Pantalla::Conteo);
        }
        if f6 {
            self.ir_a(Pantalla::Auditoria);
        }
        if f7 {
            self.ir_a(Pantalla::Clientes);
        }
        if f8 {
            self.ir_a(Pantalla::Reportes);
        }
        if f9 {
            self.ir_a(Pantalla::Caja);
        }
        if f10 {
            self.ir_a(Pantalla::Config);
        }
        if f4 && !self.mostrar_cobro && !self.venta.is_empty() {
            if self.caja.abierta {
                self.mostrar_cobro = true;
                self.abrir_cobro();
            } else {
                self.mostrar_abrir_caja = true;
            }
        }
        if enter && self.mostrar_cobro {
            self.cobrar();
            return;
        }
        // + / − con el ticket a la vista: una pieza más o menos del último.
        if (plus || minus)
            && self.consulta.is_empty()
            && self.pantalla == Pantalla::Venta
            && !self.mostrar_cobro
            && self.cantidad_linea.is_none()
            && !self.venta.is_empty()
        {
            let i = self.venta.lines.len() - 1;
            if plus {
                let sku = self.venta.lines[i].sku.clone();
                let cabe = self
                    .resultados
                    .iter()
                    .find(|p| p.sku == sku)
                    .filter(|p| p.track_inventory)
                    .map(|p| {
                        let en: i64 = self
                            .venta
                            .lines
                            .iter()
                            .filter(|l| l.sku == sku)
                            .map(|l| l.qty.millis())
                            .sum();
                        en + 1_000 <= p.stock_millis
                    })
                    .unwrap_or(true);
                if cabe {
                    self.venta.change_qty(i, Quantity::from_millis(1_000));
                }
            } else {
                self.venta.change_qty(i, Quantity::from_millis(-1_000));
            }
        }
        if supr
            && self.consulta.is_empty()
            && !self.venta.is_empty()
            && self.pantalla == Pantalla::Venta
            && !self.mostrar_cobro
        {
            let ultimo = self.venta.lines.len() - 1;
            self.venta.remove_line(ultimo);
        }
        if enter
            && !self.consulta.trim().is_empty()
            && self.pantalla == Pantalla::Venta
            && !self.mostrar_cobro
        {
            let termino = self.consulta.trim().to_string();
            // "12*" = las siguientes piezas son 12. El escáner viene después.
            if let Some(n) = cantidad_desde_atajo(&termino) {
                self.venta_cant = n;
                self.consulta.clear();
                self.foco_busqueda = true;
                return;
            }
            // Primero lo trata como código de barras; si no, como texto.
            if let Cuerpo::Productos(p) = self.ejecutar(Comando::ProductoPorCodigo {
                codigo: termino.clone(),
            }) {
                if let Some(producto) = p.first() {
                    let producto = producto.clone();
                    self.agregar_producto(&producto);
                    self.consulta.clear();
                    self.buscar("");
                    return;
                }
            }
            self.buscar(&termino);
            if self.resultados.len() == 1 {
                let p = self.resultados[0].clone();
                self.agregar_producto(&p);
                self.consulta.clear();
                self.buscar("");
            }
        }
    }

    // -------------------------------------------------------------- dibujo --

/// Una entrada de la barra lateral. En modo rail sólo se ve el icono; el
/// nombre queda en la pista y en el nombre accesible del botón.
fn nav_lateral(
    ui: &mut egui::Ui,
    rail: bool,
    activa: bool,
    nombre: &str,
    atajo: &str,
    icono: Icono,
) -> bool {
    let alto = if rail { 42.0 } else { 40.0 };
    let ancho = ui.available_width();
    let (rect, respuesta) =
        ui.allocate_exact_size(egui::vec2(ancho, alto), egui::Sense::click());
    respuesta.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            if atajo.is_empty() {
                nombre.to_string()
            } else {
                format!("{nombre} ({atajo})")
            },
        )
    });

    if activa {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(11), theme::lima());
    } else if respuesta.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(11),
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
        );
    }

    let tinta = if activa {
        theme::tinta_lima()
    } else {
        theme::barra_lateral_texto()
    };
    if rail {
        icono.dibujar(ui.painter(), rect.center(), 20.0, tinta);
    } else {
        let centro = egui::pos2(rect.left() + 21.0, rect.center().y);
        icono.dibujar(ui.painter(), centro, 18.0, tinta);
        ui.painter().text(
            egui::pos2(rect.left() + 40.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            nombre,
            egui::FontId::proportional(12.5),
            tinta,
        );
        if !atajo.is_empty() {
            ui.painter().text(
                egui::pos2(rect.right() - 10.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                atajo,
                egui::FontId::proportional(9.0),
                if activa {
                    egui::Color32::from_rgb(0x4C, 0x66, 0x4F)
                } else {
                    theme::barra_lateral_suave()
                },
            );
        }
    }

    if rail {
        respuesta.on_hover_text(if atajo.is_empty() {
            nombre.to_string()
        } else {
            format!("{nombre} · {atajo}")
        })
    } else {
        respuesta
    }
    .clicked()
}

    fn barra_superior(&mut self, ctx: &egui::Context) {
        if self.pantalla == Pantalla::Login {
            return;
        }

        // A 1024×600 la barra se convierte en un rail de iconos. El corte se
        // toma del ancho de la ventana, no del panel: así no cambia de forma
        // a media pantalla mientras se redimensiona.
        let rail = ctx.screen_rect().width() <= 1180.0;
        let ancho = if rail { 64.0 } else { 212.0 };

        let mut ir_a: Option<Pantalla> = None;
        let mut salir = false;

        egui::SidePanel::left("barra")
            .resizable(false)
            .exact_width(ancho)
            .frame(
                egui::Frame::new()
                    .fill(theme::barra_lateral())
                    .inner_margin(egui::Margin::symmetric(if rail { 8 } else { 14 }, 14)),
            )
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 3.0);

                // --- marca ---
                if rail {
                    ui.vertical_centered(|ui| {
                        let (r, _) = ui.allocate_exact_size(
                            egui::vec2(40.0, 40.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            r,
                            egui::CornerRadius::same(13),
                            theme::lima(),
                        );
                        Icono::Tienda.dibujar(
                            ui.painter(),
                            r.center(),
                            22.0,
                            theme::tinta_lima(),
                        );
                    });
                } else {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(
                            egui::vec2(40.0, 40.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            r,
                            egui::CornerRadius::same(13),
                            theme::lima(),
                        );
                        Icono::Tienda.dibujar(
                            ui.painter(),
                            r.center(),
                            22.0,
                            theme::tinta_lima(),
                        );
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(cortar(&self.config.tienda, 18))
                                    .size(15.0)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            );
                            ui.label(
                                egui::RichText::new("Punto de venta")
                                    .size(10.0)
                                    .color(theme::barra_lateral_suave()),
                            );
                        });
                    });
                }
                ui.add_space(if rail { 16.0 } else { 20.0 });

                // --- navegación ---
                let alto_pie = if rail { 82.0 } else { 158.0 };
                let alto_nav = (ui.available_height() - alto_pie).max(120.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), alto_nav),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("nav_lateral")
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                            )
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                for (inicio, fin, titulo) in
                                    [(0usize, 5usize, "OPERACIÓN"), (5, 8, "ANÁLISIS"), (8, 9, "SISTEMA")]
                                {
                                    let hay = NAV[inicio..fin]
                                        .iter()
                                        .any(|(p, _, _, _)| self.puede_ver(*p));
                                    if !hay {
                                        continue;
                                    }
                                    if !rail {
                                        ui.add_space(10.0);
                                        ui.label(
                                            egui::RichText::new(titulo)
                                                .size(9.0)
                                                .strong()
                                                .color(theme::barra_lateral_suave()),
                                        );
                                        ui.add_space(4.0);
                                    }
                                    for (pantalla, nombre, atajo, icono) in
                                        NAV[inicio..fin].iter().copied()
                                    {
                                        if !self.puede_ver(pantalla) {
                                            continue;
                                        }
                                        if Self::nav_lateral(
                                            ui,
                                            rail,
                                            self.pantalla == pantalla,
                                            nombre,
                                            atajo,
                                            icono,
                                        ) {
                                            ir_a = Some(pantalla);
                                        }
                                    }
                                }
                            });
                    },
                );

                // --- pie: turno, usuario y salida ---
                ui.add_space(8.0);
                if rail {
                    ui.vertical_centered(|ui| {
                        // La caja, en una pastilla que dice su estado de un
                        // golpe: verde si está abierta, ámbar si no.
                        let color = if self.caja.abierta {
                            theme::lima()
                        } else {
                            theme::advertencia()
                        };
                        let (r, resp) = ui.allocate_exact_size(
                            egui::vec2(42.0, 30.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            r,
                            egui::CornerRadius::same(9),
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                        );
                        Icono::Billete.dibujar(
                            ui.painter(),
                            r.center() - egui::vec2(4.0, 0.0),
                            17.0,
                            theme::barra_lateral_texto(),
                        );
                        ui.painter().circle_filled(
                            r.right_center() - egui::vec2(7.0, 0.0),
                            3.0,
                            color,
                        );
                        resp.on_hover_text(if self.caja.abierta {
                            format!(
                                "Caja abierta · {}",
                                Money::from_cents(self.caja.esperado_cents).format_mxn()
                            )
                        } else {
                            "Caja cerrada · abre en F9".to_string()
                        });

                        ui.add_space(6.0);
                        if let Some(u) = self.usuario.clone() {
                            let (av, _) = ui.allocate_exact_size(
                                egui::vec2(32.0, 32.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(
                                av.center(),
                                16.0,
                                egui::Color32::from_rgb(0xDD, 0xE9, 0xD6),
                            );
                            ui.painter().text(
                                av.center(),
                                egui::Align2::CENTER_CENTER,
                                theme::iniciales_de(&u.display_name),
                                egui::FontId::proportional(11.0),
                                egui::Color32::from_rgb(0x2C, 0x44, 0x35),
                            );
                            ui.add_space(5.0);
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Salir")
                                        .size(9.5)
                                        .color(theme::barra_lateral_suave()),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .min_size(egui::vec2(44.0, 22.0)),
                            )
                            .on_hover_text("Cerrar la sesión")
                            .clicked()
                        {
                            salir = true;
                        }
                    });
                } else {
                    let ancho_pie = ui.available_width();
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14))
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            theme::barra_lateral_linea(),
                        ))
                        .corner_radius(egui::CornerRadius::same(13))
                        .inner_margin(egui::Margin::symmetric(11, 9))
                        .show(ui, |ui| {
                            ui.set_width(ancho_pie - 22.0);
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(6.0, 6.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(
                                    r.center(),
                                    3.0,
                                    if self.caja.abierta {
                                        theme::lima()
                                    } else {
                                        theme::advertencia()
                                    },
                                );
                                ui.add_space(5.0);
                                ui.label(
                                    egui::RichText::new(if self.caja.abierta {
                                        "Caja abierta"
                                    } else {
                                        "Caja cerrada"
                                    })
                                    .size(10.0)
                                    .strong()
                                    .color(if self.caja.abierta {
                                        egui::Color32::from_rgb(0xCF, 0xE3, 0xC3)
                                    } else {
                                        theme::advertencia()
                                    }),
                                );
                            });
                            ui.label(
                                egui::RichText::new(if self.caja.abierta {
                                    Money::from_cents(self.caja.esperado_cents).format_mxn()
                                } else {
                                    "Sin turno".to_string()
                                })
                                .size(18.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                            );
                            ui.label(
                                egui::RichText::new(if self.caja.abierta {
                                    "Efectivo en cajón"
                                } else {
                                    "Abre la caja en F9"
                                })
                                .size(9.0)
                                .color(theme::barra_lateral_suave()),
                            );
                        });

                    ui.add_space(8.0);
                    if let Some(u) = self.usuario.clone() {
                        ui.horizontal(|ui| {
                            let (av, _) = ui.allocate_exact_size(
                                egui::vec2(32.0, 32.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(
                                av.center(),
                                16.0,
                                egui::Color32::from_rgb(0xDD, 0xE9, 0xD6),
                            );
                            ui.painter().text(
                                av.center(),
                                egui::Align2::CENTER_CENTER,
                                theme::iniciales_de(&u.display_name),
                                egui::FontId::proportional(11.0),
                                egui::Color32::from_rgb(0x2C, 0x44, 0x35),
                            );
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(cortar(&u.display_name, 18))
                                        .size(11.5)
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                );
                                ui.label(
                                    egui::RichText::new(self.rol().label())
                                        .size(9.5)
                                        .color(theme::barra_lateral_suave()),
                                );
                            });
                        });
                    }
                    ui.add_space(6.0);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Salir")
                                    .size(10.0)
                                    .color(theme::barra_lateral_suave()),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                theme::barra_lateral_linea(),
                            ))
                            .min_size(egui::vec2(ui.available_width(), 26.0)),
                        )
                        .clicked()
                    {
                        salir = true;
                    }
                }
            });

        if salir {
            self.salir();
            return;
        }
        if let Some(p) = ir_a {
            self.ir_a(p);
        }
    }

    /// Cambia de pantalla cargando lo que esa pantalla necesita. Lo usan
    /// tanto las pestañas como los atajos de teclado.
    pub(crate) fn ir_a(&mut self, p: Pantalla) {
        if !self.puede_ver(p) {
            return;
        }
        self.pantalla = p;
        self.al_entrar(p);
    }

    pub(crate) fn rol(&self) -> pos_core::Role {
        match &self.usuario {
            Some(u) => pos_core::Role::desde_texto(&u.role),
            None => pos_core::Role::Admin,
        }
    }

    pub(crate) fn puede_ver(&self, p: Pantalla) -> bool {
        use pos_core::Role;
        match p {
            Pantalla::Login
            | Pantalla::Venta
            | Pantalla::Catalogo
            | Pantalla::Clientes
            | Pantalla::Caja => true,
            Pantalla::Pedidos | Pantalla::Conteo | Pantalla::Reportes | Pantalla::Auditoria => {
                self.rol() != Role::Cajero
            }
            Pantalla::Config => self.rol() == Role::Admin,
        }
    }

    pub(crate) fn es_cajero(&self) -> bool {
        self.usuario.is_some() && self.rol() == pos_core::Role::Cajero
    }

    pub(crate) fn es_admin(&self) -> bool {
        self.rol() == pos_core::Role::Admin
    }

    /// Carga lo que cada pantalla necesita al abrirse. Vivía repetido en la
    /// barra; ahora está en un solo lugar, así que las pestañas y los atajos
    /// de teclado hacen exactamente lo mismo.
    fn al_entrar(&mut self, p: Pantalla) {
        match p {
            Pantalla::Venta => self.foco_busqueda = true,
            Pantalla::Catalogo => {
                self.buscar("");
                self.refrescar_alertas();
                self.foco_alta = true;
            }
            Pantalla::Pedidos => self.refrescar_pedidos(),
            Pantalla::Clientes => self.refrescar_clientes(),
            Pantalla::Conteo => {
                self.refrescar_categorias();
                self.refrescar_conteo();
            }
            Pantalla::Auditoria => self.refrescar_auditoria(),
            Pantalla::Reportes => {
                self.refrescar_cajeros();
                self.refrescar_clientes();
            }
            Pantalla::Caja => self.refrescar_caja(),
            Pantalla::Config => {
                self.refrescar_respaldos();
                self.refrescar_actualizacion();
                self.refrescar_impresoras();
                self.proponer_impresora_si_falta();
                self.refrescar_fiscal();
                self.refrescar_usuarios_admin();
            }
            Pantalla::Login => {}
        }
    }

    fn pantalla_login(&mut self, ui: &mut egui::Ui) {
        // Franja de bienvenida: el nombre de la tienda sobre un degradado que
        // se va hacia el fondo de la ventana. Es la portada del programa y lo
        // que hace que al abrirlo parezca una aplicación y no un formulario.
        let (banda, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 168.0),
            egui::Sense::hover(),
        );
        theme::degradado(
            ui.painter(),
            banda,
            theme::tenir(theme::acento(), 0.28),
            theme::panel(),
            egui::CornerRadius::ZERO,
        );
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(banda), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(28.0);
                let (r, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(r.center(), 26.0, theme::tenir(theme::acento(), 0.22));
                Icono::Tienda.dibujar(ui.painter(), r.center(), 30.0, theme::acento());
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(self.config.tienda.clone())
                        .size(32.0)
                        .strong()
                        .color(theme::acento()),
                );
                ui.label(
                    egui::RichText::new(ahora_cdmx())
                        .size(14.0)
                        .color(theme::texto_suave()),
                );
            });
        });
        ui.add_space(22.0);

        // El pie se clava abajo: si no, el login es una isla de tarjetas
        // flotando en un mar de vacío y se ve inacabado.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(18.0);
            ui.label(
                egui::RichText::new("Punto de venta · licencia MIT")
                    .size(13.0)
                    .color(theme::texto_suave()),
            );
            ui.add_space(8.0);

            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                if let Some(pendiente) = self.pendiente.clone() {
                    self.teclado_pin(ui, &pendiente);
                    return;
                }

                if self.usuarios.is_empty() {
                    theme::pastilla(ui, "No hay usuarios en el sistema", theme::peligro());
                    return;
                }

                let usuarios = self.usuarios.clone();
                let filas = usuarios.len().div_ceil(2);
                let ancho_bloque = ui.available_width().clamp(320.0, 680.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ancho_bloque, 48.0 + filas as f32 * 92.0),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        // Un marco agrupa la pregunta y las personas: deja de
                        // ser cuatro tarjetas sueltas en el centro.
                        theme::marco_tarjeta(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("¿Quién atiende?").heading());
                            });
                            ui.add_space(12.0);
                            ui.columns(2, |cols| {
                                for i in 0..usuarios.len() {
                                    if tarjeta_usuario(&mut cols[i % 2], &usuarios[i]) {
                                        self.pendiente = Some(usuarios[i].clone());
                                        self.pin.clear();
                                        self.pin_error.clear();
                                    }
                                }
                            });
                        });
                    },
                );
            });
        });
    }

    fn teclado_pin(&mut self, ui: &mut egui::Ui, pendiente: &UsuarioDto) {
        // Un pad de verdad, no una fila de botones sueltos: se usa con el
        // dedo en el mostrador, así que cada tecla es del tamaño de un pulgar.
        let ancho = 340.0;
        ui.allocate_ui_with_layout(
            egui::vec2(ancho, 560.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                theme::marco_tarjeta(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
                        theme::pintar_iniciales(
                            ui.painter(),
                            r,
                            &pendiente.display_name,
                            theme::acento(),
                        );
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(&pendiente.display_name)
                                    .size(18.0)
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new("Escribe tu PIN")
                                    .size(13.0)
                                    .color(theme::texto_suave()),
                            );
                        });
                    });
                    ui.add_space(14.0);

                    egui::Frame::new()
                        .fill(theme::fondo())
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.add_sized(
                                [ui.available_width(), 44.0],
                                egui::TextEdit::singleline(&mut self.pin)
                                    .password(true)
                                    .font(egui::FontId::proportional(28.0))
                                    .horizontal_align(egui::Align::Center)
                                    .frame(false),
                            );
                        });
                    if !self.pin_error.is_empty() {
                        ui.add_space(6.0);
                        theme::pastilla(ui, &self.pin_error.clone(), theme::peligro());
                    }
                    ui.add_space(12.0);

                    let teclas = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "←", "0", "OK"];
                    let hueco = 8.0;
                    let lado = ((ui.available_width() - hueco * 2.0) / 3.0).floor();
                    for fila in teclas.chunks(3) {
                        ui.horizontal(|ui| {
                            for (i, tecla) in fila.iter().enumerate() {
                                if i > 0 {
                                    ui.add_space(hueco);
                                }
                                let respuesta = if *tecla == "OK" {
                                    ui.add_sized(
                                        [lado, 56.0],
                                        egui::Button::new(
                                            egui::RichText::new("OK")
                                                .size(18.0)
                                                .strong()
                                                .color(theme::tinta_sobre(theme::acento())),
                                        )
                                        .fill(theme::acento())
                                        .corner_radius(egui::CornerRadius::same(12)),
                                    )
                                } else if *tecla == "←" {
                                    let r = ui.add_sized(
                                        [lado, 56.0],
                                        egui::Button::new("")
                                            .corner_radius(egui::CornerRadius::same(12)),
                                    );
                                    Icono::Tache.dibujar(
                                        ui.painter(),
                                        r.rect.center(),
                                        22.0,
                                        theme::texto_suave(),
                                    );
                                    r
                                } else {
                                    ui.add_sized(
                                        [lado, 56.0],
                                        egui::Button::new(
                                            egui::RichText::new(*tecla).size(22.0).strong(),
                                        )
                                        .corner_radius(egui::CornerRadius::same(12)),
                                    )
                                };
                                if respuesta.clicked() {
                                    match *tecla {
                                        "←" => {
                                            self.pin.pop();
                                        }
                                        "OK" => self.entrar(),
                                        d => {
                                            if self.pin.chars().count() < 8 {
                                                self.pin.push_str(d);
                                            }
                                        }
                                    }
                                }
                            }
                        });
                        ui.add_space(hueco);
                    }

                    ui.add_space(4.0);
                    if ui
                        .add(egui::Button::new("Elegir otra persona").frame(false))
                        .clicked()
                    {
                        self.pendiente = None;
                        self.pin.clear();
                        self.pin_error.clear();
                    }
                });
            },
        );
    }

    fn pantalla_venta(&mut self, ctx: &egui::Context) {
        let rail = ctx.screen_rect().width() <= 1180.0;
        let (margen_h, margen_v) = if rail { (14, 10) } else { (22, 14) };

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::fondo())
                    .inner_margin(egui::Margin::symmetric(margen_h, margen_v)),
            )
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

                // --- encabezado ---
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Nueva venta")
                                .size(if rail { 21.0 } else { 25.0 })
                                .strong()
                                .color(theme::texto()),
                        );
                        let subtitulo = if self.consulta.trim().is_empty() {
                            format!(
                                "Mostrador · {} productos disponibles",
                                self.resultados.len()
                            )
                        } else {
                            format!(
                                "{} resultados para «{}»",
                                self.resultados.len(),
                                cortar(self.consulta.trim(), 24)
                            )
                        };
                        ui.label(
                            egui::RichText::new(subtitulo)
                                .size(if rail { 10.5 } else { 11.5 })
                                .color(theme::texto_suave()),
                        );
                    });
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if self.caja.abierta {
                                pastilla_estado(
                                    ui,
                                    "Caja abierta",
                                    theme::acento(),
                                    theme::lima_tenue(),
                                );
                            } else {
                                pastilla_estado(
                                    ui,
                                    "Caja cerrada",
                                    theme::advertencia(),
                                    theme::tenir(theme::advertencia(), 0.13),
                                );
                            }
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!("{} en ticket", self.piezas_ticket()))
                                    .size(9.5)
                                    .color(theme::texto_suave()),
                            );
                        },
                    );
                });

                ui.add_space(if rail { 4.0 } else { 7.0 });

                let alto = ui.available_height().max(320.0);
                let ancho_ticket = if rail { 320.0 } else { 368.0 };
                let separacion = if rail { 12.0 } else { 16.0 };
                let ancho_izq = (ui.available_width() - ancho_ticket - separacion).max(300.0);
                ui.spacing_mut().item_spacing.x = separacion;

                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ancho_izq, alto),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            self.columna_catalogo(ui, rail);
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(ancho_ticket, alto),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            self.panel_ticket(ui, rail);
                        },
                    );
                });
            });
    }

    fn piezas_ticket(&self) -> String {
        let millis: i64 = self.venta.lines.iter().map(|l| l.qty.millis()).sum();
        Quantity::from_millis(millis).format()
    }

    fn columna_catalogo(&mut self, ui: &mut egui::Ui, rail: bool) {
        ui.set_width(ui.available_width());

        // --- caja cerrada: se dice antes de buscar, no después de cobrar ---
        if !self.caja.abierta {
            egui::Frame::new()
                .fill(theme::tenir(theme::advertencia(), 0.12))
                .corner_radius(egui::CornerRadius::same(11))
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    theme::tenir(theme::advertencia(), 0.32),
                ))
                .inner_margin(egui::Margin::symmetric(12, 5))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(
                            egui::vec2(16.0, 16.0),
                            egui::Sense::hover(),
                        );
                        Icono::Candado.dibujar(
                            ui.painter(),
                            r.center(),
                            15.0,
                            theme::advertencia(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Caja cerrada: abre la caja para cobrar.")
                                .size(10.5)
                                .strong()
                                .color(theme::advertencia()),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new("Abrir caja")
                                                .size(10.0)
                                                .strong()
                                                .color(theme::tinta_sobre(theme::acento())),
                                        )
                                        .fill(theme::acento())
                                        .stroke(egui::Stroke::NONE)
                                        .corner_radius(egui::CornerRadius::same(7))
                                        .min_size(egui::vec2(0.0, 24.0)),
                                    )
                                    .clicked()
                                {
                                    self.mostrar_abrir_caja = true;
                                }
                            },
                        );
                    });
                });
            ui.add_space(6.0);
        }

        // --- búsqueda y cantidad ---
        let mut buscar_termino: Option<String> = None;
        let mut pedir_foco = self.foco_busqueda;
        let alto_busca = if rail { 32.0 } else { 36.0 };
        egui::Frame::new()
            .fill(theme::tarjeta())
            .stroke(egui::Stroke::new(1.0_f32, theme::borde()))
            .corner_radius(egui::CornerRadius::same(if rail { 12 } else { 14 }))
            .inner_margin(egui::Margin::symmetric(if rail { 12 } else { 15 }, 5))
            .shadow(theme::sombra())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(
                        egui::vec2(20.0, 20.0),
                        egui::Sense::hover(),
                    );
                    Icono::Buscar.dibujar(
                        ui.painter(),
                        r.center(),
                        19.0,
                        theme::acento(),
                    );
                    ui.add_space(6.0);

                    let reserva = if rail { 92.0 } else { 196.0 };
                    let ancho_busca = (ui.available_width() - reserva).max(130.0);
                    let busca = ui.add_sized(
                        [ancho_busca, alto_busca],
                        egui::TextEdit::singleline(&mut self.consulta)
                            .hint_text("Busca por nombre, código o escanea…")
                            .font(egui::FontId::proportional(if rail { 13.0 } else { 14.5 }))
                            .frame(false),
                    );
                    if busca.changed() {
                        buscar_termino = Some(self.consulta.clone());
                    }
                    if pedir_foco {
                        busca.request_focus();
                        pedir_foco = false;
                    }

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if !rail {
                                for n in ["24", "12", "6"] {
                                    let activo = self.venta_cant.trim() == n;
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(n)
                                                    .size(9.5)
                                                    .strong()
                                                    .color(if activo {
                                                        theme::tinta_sobre(theme::acento())
                                                    } else {
                                                        theme::texto_suave()
                                                    }),
                                            )
                                            .fill(if activo {
                                                theme::acento()
                                            } else {
                                                theme::tarjeta_alta()
                                            })
                                            .stroke(egui::Stroke::NONE)
                                            .corner_radius(egui::CornerRadius::same(6))
                                            .min_size(egui::vec2(25.0, 24.0)),
                                        )
                                        .on_hover_text(format!("Agregar de {n} en {n}"))
                                        .clicked()
                                    {
                                        self.venta_cant = n.to_string();
                                    }
                                }
                                ui.add_space(4.0);
                            }
                            ui.add_sized(
                                [44.0, 26.0],
                                egui::TextEdit::singleline(&mut self.venta_cant)
                                    .hint_text("1")
                                    .font(egui::FontId::proportional(12.0)),
                            )
                            .on_hover_text("Cuántas piezas se agregan al picar un producto");
                            ui.label(
                                egui::RichText::new("Cant.")
                                    .size(9.0)
                                    .color(theme::texto_suave()),
                            );
                        },
                    );
                });
            });
        self.foco_busqueda = false;
        if let Some(t) = buscar_termino {
            self.buscar(&t);
        }

        ui.add_space(if rail { 5.0 } else { 7.0 });

        // --- filtros reales del mostrador ---
        let n_recientes = self
            .resultados
            .iter()
            .filter(|p| self.recientes.iter().any(|(sku, _)| sku == &p.sku))
            .count();
        let n_agotan = self
            .resultados
            .iter()
            .filter(|p| p.track_inventory && p.stock_millis <= p.min_stock_millis)
            .count();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if chip_venta(ui, rail, self.filtro_venta == "todo", "Todo", None) {
                self.filtro_venta = "todo".into();
            }
            if chip_venta(
                ui,
                rail,
                self.filtro_venta == "recientes",
                "Recientes",
                Some(n_recientes),
            ) {
                self.filtro_venta = "recientes".into();
            }
            if chip_venta(
                ui,
                rail,
                self.filtro_venta == "agotan",
                "Se agotan",
                Some(n_agotan),
            ) {
                self.filtro_venta = "agotan".into();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let total = self.resultados.len();
                ui.label(
                    egui::RichText::new(if total == 1 {
                        "1 artículo".to_string()
                    } else {
                        format!("{total} artículos")
                    })
                    .size(9.5)
                    .color(theme::texto_suave()),
                );
            });
        });

        ui.add_space(if rail { 5.0 } else { 7.0 });

        // --- lo que queda para la tabla ---
        let alto_tabla = ui.available_height().max(160.0);
        let filtro = self.filtro_venta.clone();
        let visibles: Vec<ProductoDto> = self
            .resultados
            .iter()
            .filter(|p| match filtro.as_str() {
                "recientes" => self.recientes.iter().any(|(sku, _)| sku == &p.sku),
                "agotan" => p.track_inventory && p.stock_millis <= p.min_stock_millis,
                _ => true,
            })
            .cloned()
            .collect();

        let seleccion = self
            .producto_sel
            .clone()
            .filter(|s| visibles.iter().any(|p| &p.sku == s))
            .or_else(|| visibles.first().map(|p| p.sku.clone()));

        let alto_cabecera = if rail { 28.0 } else { 32.0 };
        let alto_feedback = if rail { 28.0 } else { 32.0 };
        let alto_pie = if rail { 0.0 } else { 26.0 };
        let alto_filas =
            (alto_tabla - alto_cabecera - alto_feedback - alto_pie - 2.0).max(90.0);

        let mut agregar: Option<ProductoDto> = None;
        let mut seleccionar: Option<String> = None;

        egui::Frame::new()
            .fill(theme::tarjeta())
            .stroke(egui::Stroke::new(1.0_f32, theme::borde()))
            .corner_radius(egui::CornerRadius::same(if rail { 13 } else { 16 }))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let ancho = ui.available_width();
                let columnas = columnas_venta(ancho, rail);
                cabecera_venta(ui, ancho, alto_cabecera, columnas, rail);

                let alto_fila = if rail { 53.0 } else { 54.0 };
                ui.allocate_ui_with_layout(
                    egui::vec2(ancho, alto_filas),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let vista = ui.max_rect();
                        // Lo que sobra de la última fila: se tapa con el
                        // degradado y se reserva al final del scroll para que
                        // al bajar del todo la última fila no quede cortada.
                        let resto = (vista.height() % alto_fila).max(0.0);
                        let sobra =
                            visibles.len() as f32 * alto_fila > vista.height() + 0.5;
                        egui::ScrollArea::vertical()
                            .id_salt("catalogo_venta")
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                            )
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if visibles.is_empty() {
                                    let apagado = theme::tenir(theme::texto_suave(), 0.45);
                                    ui.add_space(28.0);
                                    ui.vertical_centered(|ui| {
                                        Icono::CajaProducto.grande(ui, 38.0, apagado);
                                        ui.add_space(6.0);
                                        ui.label(
                                            egui::RichText::new("No hay productos aquí")
                                                .size(12.5)
                                                .strong()
                                                .color(theme::texto_suave()),
                                        );
                                        ui.label(
                                            egui::RichText::new(if filtro == "todo" {
                                                "Escribe otro nombre o da de alta en F3 Catálogo."
                                            } else {
                                                "Prueba con otro filtro o con Todo."
                                            })
                                            .size(10.0)
                                            .color(theme::texto_suave()),
                                        );
                                    });
                                    return;
                                }
                                for p in &visibles {
                                    let en_ticket: i64 = self
                                        .venta
                                        .lines
                                        .iter()
                                        .filter(|l| l.sku == p.sku)
                                        .map(|l| l.qty.millis())
                                        .sum();
                                    let (clic, sumar) = fila_venta(
                                        ui,
                                        p,
                                        rail,
                                        seleccion.as_deref() == Some(p.sku.as_str()),
                                        en_ticket,
                                        columnas,
                                    );
                                    if clic {
                                        seleccionar = Some(p.sku.clone());
                                    }
                                    if sumar {
                                        agregar = Some(p.clone());
                                    }
                                }
                                if sobra {
                                    ui.add_space(resto);
                                }
                            });

                        // Si quedan productos abajo, la última fila se corta a
                        // media altura. Se tapa con un degradado al color de la
                        // tarjeta: la fila no se ve rota y el scrollbar avisa
                        // que hay más.
                        if sobra {
                            desvanecer_inferior(
                                ui,
                                vista,
                                if rail { 12.0 } else { 14.0 },
                                resto,
                            );
                        }
                    },
                );

                // --- feedback de la última acción ---
                let undo = barra_feedback_venta(
                    ui,
                    ancho,
                    alto_feedback,
                    self.feedback_venta.as_ref(),
                    rail,
                );
                if undo {
                    self.deshacer_feedback();
                }

                if !rail {
                    pie_tabla_venta(ui, ancho, alto_pie, visibles.len(), self.resultados.len());
                }
            });

        if let Some(sku) = seleccionar {
            self.producto_sel = Some(sku);
        } else if agregar.is_none() {
            self.producto_sel = seleccion;
        }
        if let Some(p) = agregar {
            self.agregar_producto(&p);
        }
    }

    fn panel_ticket(&mut self, ui: &mut egui::Ui, rail: bool) {
        let ancho = ui.available_width();
        let alto = ui.available_height().max(260.0);
        let piezas = self.piezas_ticket();

        let editando = self.cantidad_linea.is_some() || self.precio_linea.is_some();
        let alto_cabecera = if rail { 36.0 } else { 42.0 };
        let alto_mid = if editando {
            if rail { 46.0 } else { 50.0 }
        } else if self.descuento_abierto {
            if rail { 114.0 } else { 122.0 }
        } else if rail {
            58.0
        } else {
            67.0
        };
        // El recuadro de totales lleva subtotal, IVA, separador y la cifra
        // grande: 92 px no alcanzaban y la cifra se montaba sobre el IVA.
        let alto_totales = if rail { 102.0 } else { 116.0 };
        let alto_pago = if rail { 84.0 } else { 100.0 };
        let alto_lineas =
            (alto - alto_cabecera - alto_mid - alto_totales - alto_pago).max(80.0);

        egui::Frame::new()
            .fill(theme::tarjeta())
            .stroke(egui::Stroke::new(1.0_f32, theme::borde()))
            .corner_radius(egui::CornerRadius::same(if rail { 16 } else { 20 }))
            .shadow(theme::sombra())
            .show(ui, |ui| {
                ui.set_width(ancho);
                ui.set_min_height(alto);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                // --- cabecera ---
                let (cab, _) = ui.allocate_exact_size(
                    egui::vec2(ancho, alto_cabecera),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(
                    cab,
                    egui::CornerRadius {
                        nw: if rail { 16 } else { 20 },
                        ne: if rail { 16 } else { 20 },
                        sw: 0,
                        se: 0,
                    },
                    theme::tarjeta(),
                );
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(cab.left(), cab.bottom() - 1.0),
                        egui::vec2(cab.width(), 1.0),
                    ),
                    egui::CornerRadius::ZERO,
                    theme::borde(),
                );
                let mut hui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cab.shrink2(egui::vec2(if rail { 12.0 } else { 15.0 }, 0.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let (ir, _) = hui.allocate_exact_size(
                    egui::vec2(17.0, 17.0),
                    egui::Sense::hover(),
                );
                Icono::Ticket.dibujar(
                    hui.painter(),
                    ir.center(),
                    16.0,
                    theme::acento(),
                );
                hui.add_space(7.0);
                hui.label(
                    egui::RichText::new("Ticket en curso")
                        .size(if rail { 13.0 } else { 14.0 })
                        .strong()
                        .color(theme::texto()),
                );
                hui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let etiqueta = if piezas == "1" {
                            "1 pieza".to_string()
                        } else {
                            format!("{piezas} piezas")
                        };
                        badge_piezas(ui, &etiqueta);
                    },
                );

                // --- líneas ---
                let mut borrar: Option<usize> = None;
                let mut cambiar: Option<(usize, i64)> = None;
                let mut precio: Option<usize> = None;
                let mut editar_cant: Option<usize> = None;
                let sku_nuevo = self
                    .feedback_venta
                    .as_ref()
                    .and_then(|f| f.sku.clone());

                ui.allocate_ui_with_layout(
                    egui::vec2(ancho, alto_lineas),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("lineas_venta")
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                            )
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 2.0;
                                if self.venta.is_empty() {
                                    ui.add_space(if rail { 16.0 } else { 26.0 });
                                    ui.vertical_centered(|ui| {
                                        let apagado =
                                            theme::tenir(theme::texto_suave(), 0.45);
                                        Icono::Ticket.grande(ui, 38.0, apagado);
                                        ui.add_space(6.0);
                                        ui.label(
                                            egui::RichText::new("Todavía no hay nada")
                                                .size(12.0)
                                                .strong()
                                                .color(theme::texto_suave()),
                                        );
                                        ui.label(
                                            egui::RichText::new(
                                                "Escanea o busca un producto para empezar.",
                                            )
                                            .size(9.5)
                                            .color(theme::texto_suave()),
                                        );
                                    });
                                    return;
                                }

                                for i in 0..self.venta.lines.len() {
                                    let descripcion =
                                        self.venta.lines[i].description.clone();
                                    let qty_txt = self.venta.lines[i].qty.format();
                                    let unitario =
                                        self.venta.lines[i].unit_price.format_mxn();
                                    let importe = self.venta.lines[i].gross().format_mxn();
                                    let descuento = self.venta.lines[i].discount.cents();
                                    let sku_linea = self.venta.lines[i].sku.clone();
                                    let nueva = sku_nuevo.as_deref() == Some(sku_linea.as_str());

                                    let alto_linea = if rail { 34.0 } else { 37.0 };
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), alto_linea),
                                        egui::Sense::click(),
                                    );
                                    if nueva {
                                        ui.painter().rect_filled(
                                            rect,
                                            egui::CornerRadius::same(8),
                                            theme::lima_tenue(),
                                        );
                                    } else if resp.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            egui::CornerRadius::same(8),
                                            theme::superficie_suave(),
                                        );
                                    }
                                    let mut fui = ui.new_child(
                                        egui::UiBuilder::new()
                                            .max_rect(rect.shrink2(egui::vec2(5.0, 0.0)))
                                            .layout(
                                                egui::Layout::left_to_right(
                                                    egui::Align::Center,
                                                ),
                                            ),
                                    );
                                    fui.spacing_mut().item_spacing.x = 0.0;

                                    if badge_cantidad(&mut fui, &qty_txt, rail) {
                                        editar_cant = Some(i);
                                    }
                                    fui.add_space(8.0);

                                    let ancho_restante = fui.available_width();
                                    let ancho_iconos = if rail { 80.0 } else { 88.0 };
                                    let ancho_total = if rail { 46.0 } else { 50.0 };
                                    let ancho_nombre = (ancho_restante
                                        - ancho_iconos
                                        - ancho_total
                                        - 4.0)
                                        .max(62.0);
                                    fui.allocate_ui_with_layout(
                                        egui::vec2(ancho_nombre, alto_linea),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.add_space(if rail { 4.0 } else { 5.0 });
                                            ui.label(
                                                egui::RichText::new(cortar(
                                                    &descripcion,
                                                    if rail { 22 } else { 26 },
                                                ))
                                                .size(if rail { 10.5 } else { 11.5 })
                                                .strong()
                                                .color(theme::texto()),
                                            );
                                            let meta = if descuento > 0 {
                                                format!(
                                                    "{unitario} c/u · desc -{}",
                                                    Money::from_cents(descuento)
                                                        .format_mxn()
                                                )
                                            } else {
                                                format!("{unitario} c/u")
                                            };
                                            ui.label(
                                                egui::RichText::new(cortar(&meta, 30))
                                                    .size(if rail { 8.5 } else { 9.0 })
                                                    .color(theme::texto_suave()),
                                            );
                                        },
                                    );

                                    fui.allocate_ui_with_layout(
                                        egui::vec2(
                                            ancho_iconos + ancho_total,
                                            alto_linea,
                                        ),
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if icono_linea(
                                                ui,
                                                Icono::Basura,
                                                theme::texto_suave(),
                                                "Quitar del ticket",
                                            ) {
                                                borrar = Some(i);
                                            }
                                            if icono_linea(
                                                ui,
                                                Icono::Etiqueta,
                                                theme::texto_suave(),
                                                "Cambiar el precio (queda en la bitácora)",
                                            ) {
                                                precio = Some(i);
                                            }
                                            let sin_mas = self
                                                .resultados
                                                .iter()
                                                .find(|p| p.sku == sku_linea)
                                                .filter(|p| p.track_inventory)
                                                .map(|p| {
                                                    let en: i64 = self
                                                        .venta
                                                        .lines
                                                        .iter()
                                                        .filter(|l| l.sku == sku_linea)
                                                        .map(|l| l.qty.millis())
                                                        .sum();
                                                    en >= p.stock_millis
                                                })
                                                .unwrap_or(false);
                                            if icono_linea(
                                                ui,
                                                Icono::Mas,
                                                if sin_mas {
                                                    theme::texto_suave()
                                                } else {
                                                    theme::acento()
                                                },
                                                if sin_mas {
                                                    "No hay más en el anaquel"
                                                } else {
                                                    "Uno más"
                                                },
                                            ) {
                                                cambiar = Some((i, 1));
                                            }
                                            if icono_linea(
                                                ui,
                                                Icono::Menos,
                                                theme::acento(),
                                                "Uno menos",
                                            ) {
                                                cambiar = Some((i, -1));
                                            }
                                            ui.label(
                                                egui::RichText::new(&importe)
                                                    .size(if rail { 10.5 } else { 11.5 })
                                                    .strong()
                                                    .color(theme::texto()),
                                            );
                                        },
                                    );
                                }
                            });
                    },
                );

                if let Some(i) = borrar {
                    self.quitar_linea_ticket(i);
                }
                if let Some((i, cuanto)) = cambiar {
                    self.cambiar_linea_ticket(i, cuanto);
                }
                if let Some(i) = precio {
                    if i < self.venta.lines.len() {
                        self.precio_linea = Some(i);
                        self.cantidad_linea = None;
                        self.precio_linea_txt = format!(
                            "{:.2}",
                            self.venta.lines[i].unit_price.cents() as f64 / 100.0
                        );
                    }
                }
                if let Some(i) = editar_cant {
                    if i < self.venta.lines.len() {
                        self.cantidad_linea = Some(i);
                        self.precio_linea = None;
                        self.cantidad_linea_txt = self.venta.lines[i].qty.format();
                    }
                }

                // --- descuento, cliente o editor ---
                ui.allocate_ui_with_layout(
                    egui::vec2(ancho, alto_mid),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let margen = if rail { 12.0 } else { 14.0 };
                        ui.add_space(if rail { 3.0 } else { 4.0 });
                        ui.horizontal(|ui| {
                            ui.add_space(margen);
                            ui.vertical(|ui| {
                                ui.set_width((ancho - margen * 2.0).max(100.0));
                                if let Some(i) = self.cantidad_linea {
                                    editor_cantidad_ticket(self, ui, i, rail);
                                } else if let Some(i) = self.precio_linea {
                                    editor_precio_ticket(self, ui, i, rail);
                                } else {
                                    if boton_descuento_ticket(
                                        ui,
                                        rail,
                                        self.descuento_abierto,
                                    ) {
                                        self.descuento_abierto = !self.descuento_abierto;
                                    }
                                    if self.descuento_abierto {
                                        ui.add_space(3.0);
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            ui.label(
                                                egui::RichText::new("Producto")
                                                    .size(9.0)
                                                    .color(theme::texto_suave()),
                                            );
                                            ui.add_sized(
                                                [58.0, 24.0],
                                                egui::TextEdit::singleline(
                                                    &mut self.descuento_linea,
                                                )
                                                .hint_text("0.00")
                                                .font(egui::FontId::proportional(10.0)),
                                            );
                                            ui.label(
                                                egui::RichText::new("Ticket")
                                                    .size(9.0)
                                                    .color(theme::texto_suave()),
                                            );
                                            ui.add_sized(
                                                [58.0, 24.0],
                                                egui::TextEdit::singleline(
                                                    &mut self.descuento_global,
                                                )
                                                .hint_text("0.00")
                                                .font(egui::FontId::proportional(10.0)),
                                            );
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Aplicar")
                                                            .size(9.0)
                                                            .strong()
                                                            .color(theme::acento()),
                                                    )
                                                    .fill(theme::acento_tenue())
                                                    .stroke(egui::Stroke::NONE)
                                                    .corner_radius(
                                                        egui::CornerRadius::same(6),
                                                    )
                                                    .min_size(egui::vec2(0.0, 24.0)),
                                                )
                                                .clicked()
                                            {
                                                self.aplicar_descuento_linea();
                                                self.venta.global_discount =
                                                    Money::parse(&self.descuento_global)
                                                        .unwrap_or(Money::ZERO)
                                                        .min(self.venta.gross_subtotal());
                                            }
                                        });
                                    }
                                    ui.add_space(if rail { 2.0 } else { 4.0 });
                                    let ancho_fila = ui.available_width();
                                    ui.horizontal(|ui| {
                                        // La etiqueta necesita un ancho fijo: si
                                        // se deja al flujo, el combo se dibuja
                                        // encima de la última letra. Se reserva
                                        // el rectángulo completo a mano.
                                        let (rect_etiqueta, _) = ui.allocate_exact_size(
                                            egui::vec2(if rail { 44.0 } else { 52.0 }, 24.0),
                                            egui::Sense::hover(),
                                        );
                                        texto_en_rect(
                                            ui,
                                            rect_etiqueta,
                                            "Cliente",
                                            9.5,
                                            theme::texto_suave(),
                                            false,
                                            false,
                                        );
                                        let texto = if self.cliente_nombre.is_empty() {
                                            "Público en general".to_string()
                                        } else {
                                            self.cliente_nombre.clone()
                                        };
                                        let ancho_combo = (ancho_fila
                                            - if rail { 44.0 } else { 52.0 }
                                            - 8.0)
                                            .max(100.0);
                                        ui.scope(|ui| {
                                            ui.spacing_mut().interact_size.y = if rail {
                                                24.0
                                            } else {
                                                26.0
                                            };
                                            egui::ComboBox::from_id_salt("cliente_venta")
                                                .selected_text(
                                                    egui::RichText::new(cortar(&texto, 30))
                                                        .size(10.0),
                                                )
                                                .width(ancho_combo)
                                                .show_ui(ui, |ui| {
                                                    if ui
                                                        .selectable_label(
                                                            self.cliente_id.is_none(),
                                                            "Público en general",
                                                        )
                                                        .clicked()
                                                    {
                                                        self.cliente_id = None;
                                                        self.cliente_nombre.clear();
                                                    }
                                                    let clientes = self.clientes.clone();
                                                    for c in clientes {
                                                        let etiqueta = format!(
                                                            "{} (debe {})",
                                                            c.name,
                                                            Money::from_cents(
                                                                c.saldo_cents
                                                            )
                                                            .format_mxn()
                                                        );
                                                        if ui
                                                            .selectable_label(
                                                                self.cliente_id
                                                                    == Some(c.id),
                                                                etiqueta,
                                                            )
                                                            .clicked()
                                                        {
                                                            self.cliente_id = Some(c.id);
                                                            self.cliente_nombre =
                                                                c.name.clone();
                                                        }
                                                    }
                                                });
                                        });
                                    });
                                }
                            });
                        });
                    },
                );

                // --- totales ---
                let subtotal = self.venta.subtotal();
                let descuento_total = self.venta.discount_total();
                let iva = self.venta.tax_total();
                let total = self.venta.total();
                ui.allocate_ui_with_layout(
                    egui::vec2(ancho, alto_totales),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let margen = if rail { 12.0 } else { 14.0 };
                        ui.add_space(if rail { 3.0 } else { 5.0 });
                        ui.horizontal(|ui| {
                            ui.add_space(margen);
                            egui::Frame::new()
                                .fill(theme::superficie_suave())
                                .stroke(egui::Stroke::new(1.0_f32, theme::borde()))
                                .corner_radius(egui::CornerRadius::same(12))
                                .inner_margin(egui::Margin::symmetric(
                                    if rail { 10 } else { 12 },
                                    if rail { 6 } else { 8 },
                                ))
                                .show(ui, |ui| {
                                    ui.set_width(
                                        (ancho - margen * 2.0
                                            - if rail { 20.0 } else { 24.0 })
                                            .max(100.0),
                                    );
                                    // El marco hereda el layout horizontal del
                                    // renglón que lo contiene: sin este vertical
                                    // las tres cifras se dibujan una encima de
                                    // otra en vez de apilarse.
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 2.0;
                                        fila_ticket(ui, "Subtotal", &subtotal.format_mxn(), None);
                                        if descuento_total.cents() > 0 {
                                            fila_ticket(
                                                ui,
                                                "Descuento",
                                                &format!("-{descuento_total}"),
                                                Some(theme::peligro()),
                                            );
                                        }
                                        fila_ticket(ui, "IVA", &iva.format_mxn(), None);
                                        let (r, _) = ui.allocate_exact_size(
                                            egui::vec2(ui.available_width(), 1.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(
                                            r,
                                            egui::CornerRadius::ZERO,
                                            theme::borde(),
                                        );
                                        ui.add_space(3.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new("TOTAL")
                                                    .size(9.0)
                                                    .strong()
                                                    .color(theme::texto_suave()),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(
                                                    egui::Align::Center,
                                                ),
                                                |ui| {
                                                    ui.label(
                                                        egui::RichText::new(total.format_mxn())
                                                            .size(if rail {
                                                                27.0
                                                            } else {
                                                                31.0
                                                            })
                                                            .strong()
                                                            .color(theme::acento()),
                                                    );
                                                },
                                            );
                                        });
                                    });
                                });
                        });
                    },
                );

                // --- cobrar ---
                let hay_lineas = !self.venta.is_empty();
                ui.allocate_ui_with_layout(
                    egui::vec2(ancho, alto_pago),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let margen = if rail { 12.0 } else { 14.0 };
                        ui.add_space(if rail { 4.0 } else { 5.0 });
                        ui.horizontal(|ui| {
                            ui.add_space(margen);
                            ui.vertical(|ui| {
                                ui.set_width((ancho - margen * 2.0).max(100.0));
                                let se_puede = self.caja.abierta && hay_lineas;
                                let cobrar = egui::Button::new(
                                    egui::RichText::new("COBRAR   F4")
                                        .size(if rail { 12.5 } else { 13.5 })
                                        .strong()
                                        .color(theme::tinta_lima()),
                                )
                                .fill(theme::lima())
                                .stroke(egui::Stroke::new(1.0_f32, theme::lima_oscuro()))
                                .corner_radius(egui::CornerRadius::same(11))
                                .min_size(egui::vec2(
                                    ui.available_width(),
                                    if rail { 38.0 } else { 44.0 },
                                ));
                                let resp = ui
                                    .add_enabled(se_puede || !self.caja.abierta, cobrar)
                                    .on_hover_text(if !self.caja.abierta {
                                        "Abre la caja para cobrar"
                                    } else if hay_lineas {
                                        "Cobrar  F4"
                                    } else {
                                        "Agrega un producto para cobrar"
                                    });
                                if resp.clicked() {
                                    if self.caja.abierta {
                                        self.mostrar_cobro = true;
                                        self.abrir_cobro();
                                    } else {
                                        self.mostrar_abrir_caja = true;
                                    }
                                }
                                ui.add_space(5.0);
                                let cancelar = ui.add_sized(
                                    [
                                        ui.available_width(),
                                        if rail { 24.0 } else { 28.0 },
                                    ],
                                    egui::Button::new(
                                        egui::RichText::new("Cancelar venta")
                                            .size(if rail { 9.5 } else { 10.0 })
                                            .color(theme::texto_suave()),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0_f32, theme::borde()))
                                    .corner_radius(egui::CornerRadius::same(8)),
                                );
                                if cancelar.clicked() {
                                    self.registrar_feedback(
                                        "Venta cancelada",
                                        "El ticket quedó vacío.",
                                    );
                                    self.venta = Sale::new("");
                                    self.descuento_global.clear();
                                    self.cliente_id = None;
                                    self.cliente_nombre.clear();
                                    self.mensaje.clear();
                                }
                            });
                        });
                    },
                );
            });
    }

    fn quitar_linea_ticket(&mut self, i: usize) {
        if i >= self.venta.lines.len() {
            return;
        }
        let antes = self.venta.clone();
        let nombre = self.venta.lines[i].description.clone();
        self.venta.remove_line(i);
        self.cantidad_linea = None;
        self.precio_linea = None;
        self.feedback_venta = Some(FeedbackVenta {
            titulo: format!("{nombre} quitado"),
            detalle: "El ticket se actualizó.".into(),
            sku: None,
            antes: Some(antes),
        });
    }

    fn cambiar_linea_ticket(&mut self, i: usize, cuanto: i64) {
        if i >= self.venta.lines.len() {
            return;
        }
        let sku = self.venta.lines[i].sku.clone();
        let nombre = self.venta.lines[i].description.clone();
        let antes = self.venta.clone();
        if cuanto > 0 {
            let hay = self
                .resultados
                .iter()
                .find(|p| p.sku == sku)
                .filter(|p| p.track_inventory)
                .map(|p| p.stock_millis);
            if let Some(hay) = hay {
                let en_ticket: i64 = self
                    .venta
                    .lines
                    .iter()
                    .filter(|l| l.sku == sku)
                    .map(|l| l.qty.millis())
                    .sum();
                if en_ticket + 1_000 > hay {
                    self.mensaje = format!(
                        "No hay más '{nombre}': hay {}",
                        Quantity::from_millis(hay).format()
                    );
                    return;
                }
            }
        }
        self.venta
            .change_qty(i, Quantity::from_millis(if cuanto > 0 { 1_000 } else { -1_000 }));
        let qty = self.venta.lines[i].qty.format();
        self.feedback_venta = Some(FeedbackVenta {
            titulo: format!("{nombre} · {qty}"),
            detalle: "Cantidad actualizada.".into(),
            sku: Some(sku),
            antes: Some(antes),
        });
    }

    /// La ventana de cobro.
    ///
    /// Tres bloques, en el orden en que se usan: cuánto se debe, con qué se
    /// está pagando y qué falta (o cuánto se devuelve). El total y el cambio
    /// son lo único grande: todo lo demás acompaña.
    fn ventana_cobro(&mut self, ctx: &egui::Context, permitir_f4: bool) {
        let mut abierta = self.mostrar_cobro;
        let total = self.venta.total();
        let pagado = self.pagado();
        let falta = (total - pagado).max(Money::ZERO);
        let sobra = (pagado - total).max(Money::ZERO);
        let cambio = self.cambio();
        let fiado = self
            .pagos
            .iter()
            .any(|p| p.metodo == PaymentMethod::Credito);

        egui::Window::new("Cobrar")
            .open(&mut abierta)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::new()
                    .fill(theme::tarjeta())
                    .stroke(egui::Stroke::new(1.0_f32, theme::borde()))
                    .corner_radius(egui::CornerRadius::same(16))
                    .inner_margin(egui::Margin::same(20))
                    .shadow(theme::sombra()),
            )
            .show(ctx, |ui| {
                ui.set_min_width(600.0);

                // --- cuánto se debe ---
                //
                // La franja se mide y se pinta antes de escribir encima: si se
                // pintara al final, el degradado taparía el número. El color se
                // va yendo hacia abajo, que es lo que dice "esto es lo
                // importante" sin subir la voz.
                let (banda, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 118.0),
                    egui::Sense::hover(),
                );
                theme::degradado(
                    ui.painter(),
                    banda,
                    theme::tenir(theme::acento(), 0.20),
                    theme::tenir(theme::acento(), 0.05),
                    egui::CornerRadius::same(12),
                );
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(banda.shrink(12.0)), |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                        Icono::Billete.dibujar(ui.painter(), r.center(), 21.0, theme::acento());
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("TOTAL A PAGAR")
                                .size(14.0)
                                .strong()
                                .color(theme::acento()),
                        );
                        ui.label(theme::monto_grande(total));
                    });
                });
                ui.add_space(8.0);

                // --- con qué se está pagando ---
                //
                // Icono y combo van en la misma celda: si el icono ocupa una
                // columna, los encabezados (Método / Monto / Recibe) no caen
                // encima de lo que nombran.
                ui.label(egui::RichText::new("Cómo paga").strong());
                egui::Grid::new("pagos")
                    .num_columns(4)
                    .spacing([12.0, 10.0])
                    .min_col_width(80.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Método")
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                        ui.label(
                            egui::RichText::new("Monto")
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                        ui.label(
                            egui::RichText::new("Recibe (efectivo)")
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                        ui.label("");
                        ui.end_row();

                        let mut quitar: Option<usize> = None;
                        let n = self.pagos.len();
                        for i in 0..n {
                            let dibujo = match self.pagos[i].metodo {
                                PaymentMethod::Efectivo => Icono::Billete,
                                PaymentMethod::Tarjeta => Icono::Tarjeta,
                                PaymentMethod::Transferencia => Icono::Tienda,
                                PaymentMethod::Credito => Icono::Persona,
                                _ => Icono::Foco,
                            };
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(22.0, 22.0),
                                    egui::Sense::hover(),
                                );
                                dibujo.dibujar(ui.painter(), r.center(), 20.0, theme::acento());
                                ui.add_space(6.0);
                                egui::ComboBox::from_id_salt(("metodo", i))
                                    .selected_text(self.pagos[i].metodo.label())
                                    .width(150.0)
                                    .show_ui(ui, |ui| {
                                        for metodo in PaymentMethod::ALL {
                                            if ui
                                                .selectable_label(
                                                    self.pagos[i].metodo == metodo,
                                                    metodo.label(),
                                                )
                                                .clicked()
                                            {
                                                self.pagos[i].metodo = metodo;
                                                if metodo != PaymentMethod::Efectivo {
                                                    self.pagos[i].recibido.clear();
                                                }
                                            }
                                        }
                                    });
                            });
                            ui.add_sized(
                                [150.0, 36.0],
                                egui::TextEdit::singleline(&mut self.pagos[i].monto)
                                    .hint_text("0.00"),
                            );
                            if self.pagos[i].metodo == PaymentMethod::Efectivo {
                                ui.add_sized(
                                    [150.0, 36.0],
                                    egui::TextEdit::singleline(&mut self.pagos[i].recibido)
                                        .hint_text("exacto"),
                                );
                            } else {
                                ui.label(egui::RichText::new("—").color(theme::texto_suave()));
                            }
                            if n > 1 {
                                if ui.small_button("quitar").clicked() {
                                    quitar = Some(i);
                                }
                            } else {
                                ui.label("");
                            }
                            ui.end_row();
                        }
                        if let Some(i) = quitar {
                            self.pagos.remove(i);
                        }
                    });

                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("+ Agregar otro pago").clicked() {
                        // Lo que falta, con el método más probable.
                        self.pagos.push(FilaPago {
                            metodo: PaymentMethod::Tarjeta,
                            monto: format!("{:.2}", falta.cents() as f64 / 100.0),
                            recibido: String::new(),
                        });
                    }
                    if ui.button("Pago exacto en efectivo").clicked() {
                        let t = format!("{:.2}", total.cents() as f64 / 100.0);
                        self.pagos = vec![FilaPago {
                            metodo: PaymentMethod::Efectivo,
                            monto: t.clone(),
                            recibido: t,
                        }];
                    }
                    if ui
                        .button("Cobrar con tarjeta")
                        .on_hover_text("Cobra el total con tarjeta, sin preguntar el billete.")
                        .clicked()
                    {
                        self.pagos = vec![FilaPago {
                            metodo: PaymentMethod::Tarjeta,
                            monto: format!("{:.2}", total.cents() as f64 / 100.0),
                            recibido: String::new(),
                        }];
                        self.cobrar();
                    }
                });
                if self
                    .pagos
                    .iter()
                    .any(|p| p.metodo == PaymentMethod::Efectivo)
                {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("¿Con cuánto te pagan?")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                    ui.horizontal_wrapped(|ui| {
                        // "Exacto" = paga justo, sin cambio: lo más común con
                        // tarjeta no, con efectivo sí (y evita teclear).
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("Exacto").strong())
                                    .min_size(egui::vec2(72.0, 32.0)),
                            )
                            .on_hover_text("Recibe justo el total, sin cambio")
                            .clicked()
                        {
                            if let Some(p) = self
                                .pagos
                                .iter_mut()
                                .find(|p| p.metodo == PaymentMethod::Efectivo)
                            {
                                let t = format!("{:.2}", total.cents() as f64 / 100.0);
                                if p.monto.trim().is_empty() {
                                    p.monto = t.clone();
                                }
                                p.recibido = t;
                            }
                        }
                        for (cents, etiqueta) in DENOMINACIONES_MXN {
                            if cents < 2_000 {
                                continue;
                            }
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new(etiqueta).strong())
                                        .min_size(egui::vec2(58.0, 32.0)),
                                )
                                .clicked()
                            {
                                if let Some(p) = self
                                    .pagos
                                    .iter_mut()
                                    .find(|p| p.metodo == PaymentMethod::Efectivo)
                                {
                                    p.recibido = format!("{:.2}", cents as f64 / 100.0);
                                    if p.monto.trim().is_empty() {
                                        p.monto = format!("{:.2}", total.cents() as f64 / 100.0);
                                    }
                                }
                            }
                        }
                    });
                }
                ui.add_space(8.0);

                // --- qué falta o cuánto se devuelve ---
                theme::marco_tarjeta(ui, |ui| {
                    fila(ui, "Pagado", &pagado.format_mxn(), None);
                    if falta.cents() > 0 {
                        fila_monto(ui, "Falta por cubrir", falta.cents(), theme::peligro());
                    } else if sobra.cents() > 0 {
                        fila_monto(
                            ui,
                            "Te sobra (revisa los montos)",
                            sobra.cents(),
                            theme::peligro(),
                        );
                    } else {
                        // El cambio se entrega: va en su propia banda verde
                        // para que no se confunda con el total.
                        egui::Frame::new()
                            .fill(theme::tenir(theme::exito(), 0.13))
                            .corner_radius(egui::CornerRadius::same(10))
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(
                                        egui::vec2(18.0, 18.0),
                                        egui::Sense::hover(),
                                    );
                                    Icono::Paloma.dibujar(
                                        ui.painter(),
                                        r.center(),
                                        17.0,
                                        theme::exito(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new("Cambio")
                                            .size(15.0)
                                            .strong()
                                            .color(theme::exito()),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(theme::monto_grande(cambio));
                                        },
                                    );
                                });
                            });
                    }
                });

                // El fiado se avisa antes de cobrar, no después: una vez
                // guardada, la venta ya está hecha.
                if fiado {
                    ui.add_space(6.0);
                    let nombre = if self.cliente_nombre.is_empty() {
                        "sin cliente".to_string()
                    } else {
                        self.cliente_nombre.clone()
                    };
                    theme::marco_tarjeta(ui, |ui| {
                        ui.colored_label(
                            theme::peligro(),
                            format!("Se fiará a {nombre}. Queda anotado en la bitácora."),
                        );
                    });
                }

                ui.add_space(10.0);
                let ancho_boton = (ui.available_width() - 8.0) / 2.0;
                ui.horizontal(|ui| {
                    let confirmar = egui::Button::new(
                        egui::RichText::new("Confirmar  Enter")
                            .heading()
                            .color(theme::tinta_sobre(theme::acento())),
                    )
                    .fill(theme::acento())
                    .min_size(egui::vec2(ancho_boton, 52.0));
                    if ui.add(confirmar).clicked() {
                        self.cobrar();
                    }
                    if ui
                        .add_sized(
                            [ancho_boton, 52.0],
                            egui::Button::new(
                                egui::RichText::new("Volver  Esc").color(theme::texto()),
                            ),
                        )
                        .clicked()
                    {
                        self.mostrar_cobro = false;
                    }
                });
            });
        self.mostrar_cobro = abierta;

        if permitir_f4 && self.mostrar_cobro {
            let f4 = ctx.input(|i| i.key_pressed(egui::Key::F4));
            if f4 {
                self.cobrar();
            }
        }
    }

    // --------------------------------------------------------------- caja --

    /// El cuerpo de la pantalla que toca, dentro del panel central.
    ///
    /// El inicio de sesión se acomoda solo; las demás se recorren. Sin ese
    /// scroll, en una pantalla de 768 de alto la de reportes se cortaría y lo
    /// de hasta abajo quedaría inalcanzable. Está aparte para que las
    /// capturas dibujen exactamente lo mismo que se ve al usar el programa.
    pub(crate) fn cuerpo_de_la_pantalla(&mut self, ui: &mut egui::Ui) {
        if self.pantalla == Pantalla::Login {
            self.pantalla_login(ui);
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("pantalla")
            .auto_shrink([false, false])
            // La barra a la vista: si no se ve, nadie se entera de que
            // abajo hay más.
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| match self.pantalla {
                Pantalla::Login => unreachable!(),
                Pantalla::Catalogo => self.pantalla_catalogo(ui),
                Pantalla::Pedidos => self.pantalla_pedidos(ui),
                Pantalla::Clientes => self.pantalla_clientes(ui),
                Pantalla::Reportes => self.pantalla_reportes(ui),
                Pantalla::Caja => self.pantalla_caja(ui),
                Pantalla::Conteo => self.pantalla_conteo(ui),
                Pantalla::Config => self.pantalla_config(ui),
                Pantalla::Auditoria => self.pantalla_auditoria(ui),
                Pantalla::Venta => unreachable!(),
            });
    }

    fn pantalla_caja(&mut self, ui: &mut egui::Ui) {
        theme::titulo_pantalla(
            ui,
            "Caja",
            "El dinero del turno: con qué se abrió, qué entró, qué salió y con cuánto \
             se entrega.",
            Icono::Billete,
        );

        // --- sin caja abierta: se dice claro y se ofrece abrirla ---
        if !self.caja.abierta {
            theme::marco_tarjeta(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        r.center(),
                        26.0,
                        theme::tenir(theme::peligro(), 0.14),
                    );
                    Icono::Candado.dibujar(ui.painter(), r.center(), 28.0, theme::peligro());
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("No hay caja abierta")
                            .text_style(egui::TextStyle::Heading)
                            .strong()
                            .color(theme::peligro()),
                    );
                    ui.label(
                        egui::RichText::new("Sin caja abierta no se puede vender.")
                            .color(theme::texto_suave()),
                    );
                    ui.add_space(12.0);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Abrir caja")
                                    .color(theme::tinta_sobre(theme::acento())),
                            )
                            .fill(theme::acento())
                            .min_size(egui::vec2(180.0, 46.0)),
                        )
                        .clicked()
                    {
                        self.mostrar_abrir_caja = true;
                    }
                    ui.add_space(12.0);
                });
            });
            return;
        }

        let c = self.caja.clone();

        // --- de un vistazo ---
        ui.columns(3, |cols| {
            tarjeta_dato(
                &mut cols[0],
                "Tickets del turno",
                &c.tickets.to_string(),
                theme::texto(),
                Icono::Ticket,
            );
            tarjeta_dato(
                &mut cols[1],
                "Vendido",
                &Money::from_cents(c.vendido_cents).format_mxn(),
                theme::acento(),
                Icono::Grafica,
            );
            tarjeta_dato(
                &mut cols[2],
                "Efectivo esperado",
                &Money::from_cents(c.esperado_cents).format_mxn(),
                theme::texto(),
                Icono::Candado,
            );
        });
        ui.add_space(8.0);

        // --- cerrar: la acción del turno, arriba, no recortada ---
        theme::marco_tarjeta(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Terminar el turno").strong());
                    ui.label(
                        egui::RichText::new(
                            "Cuenta los billetes. Si no cuadra, se cierra igual y queda quién y cuánto.",
                        )
                        .size(12.0)
                        .color(theme::texto_suave()),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ancho = (ui.available_width() - 20.0).clamp(200.0, 300.0);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("CERRAR CAJA (arqueo)")
                                    .heading()
                                    .color(theme::tinta_sobre(theme::acento())),
                            )
                            .fill(theme::acento())
                            .min_size(egui::vec2(ancho, 52.0)),
                        )
                        .clicked()
                    {
                        self.mostrar_arqueo = true;
                    }
                });
            });
        });
        ui.add_space(8.0);

        // --- entradas y salidas de dinero: primero la acción ---
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Movimiento de efectivo", Some(Icono::Tarjeta));
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for (valor, etiqueta) in [
                    ("retiro", "Retiro"),
                    ("gasto", "Gasto"),
                    ("entrada", "Entrada"),
                ] {
                    if ui
                        .add(egui::SelectableLabel::new(self.mov_tipo == valor, etiqueta))
                        .clicked()
                    {
                        self.mov_tipo = valor.to_string();
                    }
                }
            });
            ui.add_space(8.0);
            // Motivos de un toque: escribir "pago al proveedor" 10 veces al
            // día cansa y cada quien lo escribe distinto en la bitácora.
            ui.label(
                egui::RichText::new("Motivo (toca uno o escribe otro)")
                    .size(12.0)
                    .color(theme::texto_suave()),
            );
            ui.horizontal_wrapped(|ui| {
                for motivo in [
                    "Pago a proveedor",
                    "Luz / agua / gas",
                    "Renta",
                    "Sueldo",
                    "Otro…",
                ] {
                    let puesto = self.mov_motivo.trim() == motivo;
                    if ui.selectable_label(puesto, motivo).clicked() {
                        if motivo == "Otro…" {
                            self.mov_motivo.clear();
                        } else {
                            self.mov_motivo = motivo.to_string();
                        }
                    }
                }
            });
            ui.add_space(4.0);
            // Monto, motivo y botón en una sola fila: en 1024x600 el botón
            // quedaba justo debajo del borde de la ventana.
            ui.columns(3, |cols| {
                campo(&mut cols[0], "Monto", 0.0, &mut self.mov_monto, "cuánto");
                campo(
                    &mut cols[1],
                    "Motivo",
                    0.0,
                    &mut self.mov_motivo,
                    "obligatorio: pago al proveedor",
                );
                cols[2].add_space(20.0);
                if cols[2]
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Registrar")
                                .color(theme::tinta_sobre(theme::acento())),
                        )
                        .fill(theme::acento())
                        .min_size(egui::vec2(160.0, 40.0)),
                    )
                    .clicked()
                {
                    self.registrar_movimiento();
                }
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Los retiros y los gastos quedan anotados en la bitácora.")
                    .size(12.0)
                    .color(theme::texto_suave()),
            );
        });
        ui.add_space(8.0);

        // --- de dónde sale: entradas y salidas, cada una con su color ---
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "De dónde sale el efectivo", Some(Icono::Billete));
            ui.add_space(4.0);
            fila_monto(ui, "Fondo inicial", c.fondo_cents, theme::texto_suave());
            fila_monto(
                ui,
                "Ventas en efectivo",
                c.ventas_efectivo_cents,
                theme::acento(),
            );
            fila_monto(ui, "Entradas", c.entradas_cents, theme::acento());
            fila_monto(ui, "Retiros", c.retiros_cents, theme::peligro());
            fila_monto(ui, "Gastos", c.gastos_cents, theme::peligro());
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("EFECTIVO EN CAJÓN")
                        .size(15.0)
                        .strong()
                        .color(theme::texto_suave()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(theme::monto_grande(Money::from_cents(c.esperado_cents)));
                });
            });
        });
    }

    fn registrar_movimiento(&mut self) {
        let monto = Money::parse(&self.mov_monto).unwrap_or(Money::ZERO);
        let comando = Comando::MovimientoCaja {
            tipo: self.mov_tipo.clone(),
            monto_cents: monto.cents(),
            motivo: self.mov_motivo.clone(),
            supervisor: None,
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando,
        }) {
            pos_api::Respuesta::Ok(_) => {
                self.mensaje = "✓ Movimiento guardado en la bitácora.".into();
                self.mov_monto.clear();
                self.mov_motivo.clear();
                self.refrescar_caja();
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
        }
    }

    fn ventana_arqueo(&mut self, ctx: &egui::Context) {
        let mut abierta = self.mostrar_arqueo;
        let esperado = self.caja.esperado_cents;
        let contado: i64 = self.arqueo.iter().map(|(v, c)| v * c).sum();

        egui::Window::new("Arqueo de caja")
            .open(&mut abierta)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(580.0);
                ui.label(egui::RichText::new("Cuenta el dinero del cajón").heading());
                ui.label(
                    egui::RichText::new(format!(
                        "Debería haber {}",
                        Money::from_cents(esperado).format_mxn()
                    ))
                    .color(theme::texto_suave()),
                );
                ui.separator();

                egui::Grid::new("arqueo")
                    .num_columns(5)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        for (valor, etiqueta) in DENOMINACIONES_MXN {
                            ui.label(egui::RichText::new(etiqueta).strong());
                            if ui
                                .add_sized([44.0, 40.0], egui::Button::new("−"))
                                .on_hover_text(format!("Uno menos de {etiqueta}"))
                                .clicked()
                            {
                                if let Some(p) = self.arqueo.iter_mut().find(|(d, _)| *d == valor) {
                                    p.1 = (p.1 - 1).max(0);
                                }
                            }
                            let cantidad = self
                                .arqueo
                                .iter()
                                .find(|(d, _)| *d == valor)
                                .map(|(_, c)| *c)
                                .unwrap_or(0);
                            ui.label(egui::RichText::new(cantidad.to_string()).monospace());
                            if ui
                                .add_sized([44.0, 40.0], egui::Button::new("+"))
                                .on_hover_text(format!("Uno más de {etiqueta}"))
                                .clicked()
                            {
                                if let Some(p) = self.arqueo.iter_mut().find(|(d, _)| *d == valor) {
                                    p.1 += 1;
                                }
                            }
                            ui.label(Money::from_cents(valor * cantidad).format_mxn());
                            ui.end_row();
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Contado").heading());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(theme::monto_grande(Money::from_cents(contado)));
                    });
                });
                fila_monto(ui, "Esperado", esperado, theme::texto_suave());
                let diferencia = contado - esperado;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Diferencia").heading());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if diferencia == 0 {
                            theme::acento()
                        } else {
                            theme::peligro()
                        };
                        ui.colored_label(
                            color,
                            egui::RichText::new(Money::from_cents(diferencia).format_mxn())
                                .heading(),
                        );
                    });
                });
                if diferencia != 0 {
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(theme::tenir(theme::peligro(), 0.12))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.colored_label(
                                theme::peligro(),
                                "No cuadra. Se cierra igual: la diferencia queda en la bitácora (quién y cuánto). No «arregles» el dinero a mano.",
                            );
                        });
                } else {
                    ui.add_space(4.0);
                    ui.colored_label(theme::exito(), "Cuadró al centavo.");
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(
                            [220.0, 52.0],
                            egui::Button::new(
                                egui::RichText::new("Cerrar caja")
                                    .heading()
                                    .color(theme::tinta_sobre(theme::acento())),
                            )
                            .fill(theme::acento()),
                        )
                        .clicked()
                    {
                        self.cerrar_caja();
                    }
                    if ui.button("Cancelar  Esc").clicked() {
                        self.mostrar_arqueo = false;
                    }
                    if ui.button("Limpiar conteo").clicked() {
                        for (_, c) in self.arqueo.iter_mut() {
                            *c = 0;
                        }
                    }
                });
            });
        self.mostrar_arqueo = abierta;
    }

    fn cerrar_caja(&mut self) {
        let piezas = self.arqueo.clone();
        let comando = Comando::CerrarCaja {
            piezas,
            supervisor: None,
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando,
        }) {
            pos_api::Respuesta::Ok(_) => {
                let contado: i64 = self.arqueo.iter().map(|(v, c)| v * c).sum();
                let esperado = self.caja.esperado_cents;
                let diferencia = contado - esperado;
                self.ultimo_cierre = Some((esperado, contado, diferencia));
                self.mensaje = if diferencia == 0 {
                    format!(
                        "Caja cerrada. Cuadró al centavo ({})",
                        Money::from_cents(esperado)
                    )
                } else if diferencia > 0 {
                    format!(
                        "Caja cerrada con SOBRANTE de {}",
                        Money::from_cents(diferencia)
                    )
                } else {
                    format!(
                        "Caja cerrada con FALTANTE de {}",
                        Money::from_cents(-diferencia)
                    )
                };
                for (_, c) in self.arqueo.iter_mut() {
                    *c = 0;
                }
                self.mostrar_arqueo = false;
                self.refrescar_caja();
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
        }
    }

    /// Imprime el corte en la térmica: para firmarlo y guardarlo en la
    /// carpeta. Si no hay impresora, queda en vista previa (como el ticket).
    fn imprimir_corte(&mut self, esperado: i64, contado: i64, diferencia: i64) {
        use pos_hw::escpos::{Align, TicketBuilder};
        let perfil = self.perfil();
        let mut t = TicketBuilder::con_perfil(perfil);
        t.align(Align::Center).bold(true);
        t.line(&self.config.tienda);
        t.bold(false).align(Align::Center);
        t.line("CORTE DE CAJA");
        t.line(&ahora_cdmx());
        if let Some(u) = &self.usuario {
            t.line(&format!("Cajero: {}", u.display_name));
        }
        t.separator();
        t.align(Align::Left);
        t.kv("Esperado", &Money::from_cents(esperado).format_mxn());
        t.kv("Contado", &Money::from_cents(contado).format_mxn());
        if diferencia == 0 {
            t.bold(true);
            t.kv("DIFERENCIA", &Money::from_cents(0).format_mxn());
            t.bold(false);
            t.line("Cuadro al centavo.");
        } else if diferencia > 0 {
            t.bold(true);
            t.kv("SOBRANTE", &Money::from_cents(diferencia).format_mxn());
            t.bold(false);
        } else {
            t.bold(true);
            t.kv("FALTANTE", &Money::from_cents(-diferencia).format_mxn());
            t.bold(false);
        }
        t.separator();
        t.line("Firma: ____________________");
        t.feed(2);
        t.cortar();
        let bytes = t.build();
        self.imprimir(bytes);
        if self.config.impresora.trim().is_empty() {
            self.mensaje = "Corte en vista previa (no hay impresora configurada)".into();
        } else {
            self.mensaje = "✓ Corte impreso para firmar y guardar.".into();
        }
    }

    fn ventana_abrir_caja(&mut self, ctx: &egui::Context) {
        let mut abierta = self.mostrar_abrir_caja;
        let mut abrio = false;
        egui::Window::new("Abrir caja")
            .open(&mut abierta)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label(egui::RichText::new("¿Con cuánto dinero arrancas?").heading());
                ui.label(
                    egui::RichText::new("Es el fondo para dar cambio. Si no hay fondo, deja 0.")
                        .color(theme::texto_suave()),
                );
                ui.add_sized(
                    [ui.available_width(), 44.0],
                    egui::TextEdit::singleline(&mut self.abrir_caja_monto)
                        .hint_text("0.00")
                        .font(egui::FontId::proportional(24.0)),
                );
                ui.add_space(12.0);
                if ui
                    .add_sized(
                        [200.0, 52.0],
                        egui::Button::new(
                            egui::RichText::new("Abrir")
                                .heading()
                                .color(theme::tinta_sobre(theme::acento())),
                        )
                        .fill(theme::acento()),
                    )
                    .clicked()
                {
                    let fondo = Money::parse(&self.abrir_caja_monto).unwrap_or(Money::ZERO);
                    match self.api.ejecutar(Peticion {
                        token: self.token(),
                        comando: Comando::AbrirCaja {
                            fondo_cents: fondo.cents(),
                        },
                    }) {
                        pos_api::Respuesta::Ok(_) => {
                            // La ventana restaura su estado al terminar; si no
                            // se marca que ya abrió, la caja queda abierta
                            // pero el modal se queda encima.
                            abrio = true;
                            self.mostrar_abrir_caja = false;
                            self.abrir_caja_monto.clear();
                            self.mensaje =
                                format!("✓ Caja abierta con fondo de {fondo}. A vender (F2).");
                            self.refrescar_caja();
                        }
                        pos_api::Respuesta::Error(e) => self.mensaje = e,
                    }
                }
            });
        self.mostrar_abrir_caja = abierta && !abrio;
    }

    fn ventana_cierre(&mut self, ctx: &egui::Context) {
        let mut abierta = self.ultimo_cierre.is_some();
        let cierre = self.ultimo_cierre;
        egui::Window::new("Corte de caja")
            .open(&mut abierta)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if let Some((esperado, contado, diferencia)) = cierre {
                    ui.set_min_width(420.0);
                    ui.vertical_centered_justified(|ui| {
                        ui.label(egui::RichText::new("Resultado del corte").heading());
                    });
                    ui.separator();
                    fila_monto(ui, "Esperado", esperado, theme::texto_suave());
                    fila_monto(ui, "Contado", contado, theme::texto_suave());
                    let (etiqueta, monto, color) = if diferencia == 0 {
                        ("Diferencia", 0, theme::acento())
                    } else if diferencia > 0 {
                        ("SOBRANTE", diferencia, theme::acento())
                    } else {
                        ("FALTANTE", -diferencia, theme::peligro())
                    };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(etiqueta).heading().color(color));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(theme::monto_grande(Money::from_cents(monto)));
                        });
                    });
                    if diferencia != 0 {
                        ui.label(
                            egui::RichText::new(
                                "Quedó en la bitácora quién cerró y de cuánto fue.",
                            )
                            .size(13.0)
                            .color(theme::texto_suave()),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("Cuadró al centavo.")
                                .size(13.0)
                                .color(theme::exito()),
                        );
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized([200.0, 44.0], theme::boton_acento("Imprimir corte"))
                            .on_hover_text("Sale en la térmica para firmarlo y guardarlo")
                            .clicked()
                        {
                            self.imprimir_corte(esperado, contado, diferencia);
                        }
                        if ui
                            .add_sized([160.0, 44.0], theme::boton_suave("Aceptar"))
                            .clicked()
                        {
                            self.ultimo_cierre = None;
                            self.mostrar_abrir_caja = true;
                        }
                    });
                }
            });
        if !abierta {
            self.ultimo_cierre = None;
        }
    }

    // ----------------------------------------------------------- clientes --

    fn pantalla_clientes(&mut self, ui: &mut egui::Ui) {
        theme::titulo_pantalla(
            ui,
            "Clientes y cartera",
            "A quién le fías y cuánto te deben. Con sus datos fiscales completos, sus \
             ventas se facturan aparte y no entran en la factura global del día.",
            Icono::Persona,
        );

        // --- de un vistazo: cuánta gente y cuánto dinero ---
        let por_cobrar: i64 = self.clientes.iter().map(|c| c.saldo_cents).sum();
        let con_deuda = self.clientes.iter().filter(|c| c.saldo_cents > 0).count();
        let con_credito = self
            .clientes
            .iter()
            .filter(|c| c.disponible_cents > 0)
            .count();
        ui.columns(3, |cols| {
            tarjeta_dato(
                &mut cols[0],
                "Clientes",
                &self.clientes.len().to_string(),
                theme::acento(),
                Icono::Persona,
            );
            tarjeta_dato(
                &mut cols[1],
                "Te deben",
                &Money::from_cents(por_cobrar).format_mxn(),
                if por_cobrar > 0 {
                    theme::peligro()
                } else {
                    theme::texto()
                },
                Icono::Billete,
            );
            tarjeta_dato(
                &mut cols[2],
                "Con crédito disponible",
                &con_credito.to_string(),
                theme::texto(),
                Icono::Paloma,
            );
        });
        ui.add_space(8.0);

        // --- alta plegada: primero se mira la cartera ---
        egui::CollapsingHeader::new(
            egui::RichText::new("Cliente nuevo: nombre, teléfono y límite")
                .strong()
                .size(15.0),
        )
        .id_salt("alta_cliente")
        .default_open(false)
        .show(ui, |ui| {
            theme::marco_tarjeta(ui, |ui| {
                theme::seccion(ui, "Quién es", Some(Icono::Persona));
                ui.add_space(4.0);
                ui.columns(3, |cols| {
                    campo(
                        &mut cols[0],
                        "Nombre o razón social",
                        0.0,
                        &mut self.form_cliente.name,
                        "María López",
                    );
                    campo(
                        &mut cols[1],
                        "Teléfono",
                        0.0,
                        &mut self.form_cliente.phone,
                        "2281234567",
                    );
                    campo(
                        &mut cols[2],
                        "Límite de crédito $",
                        0.0,
                        &mut self.form_cliente_limite,
                        "0.00",
                    );
                });
                ui.add_space(8.0);

                theme::seccion(ui, "Para facturarle (opcional)", Some(Icono::Factura));
                ui.add_space(4.0);
                ui.columns(3, |cols| {
                    campo(
                        &mut cols[0],
                        "RFC",
                        0.0,
                        &mut self.form_cliente.rfc,
                        "XAXX010101000",
                    );
                    combo_clave(
                        &mut cols[1],
                        "Régimen fiscal",
                        "regimen_cli",
                        &mut self.form_cliente.regimen_fiscal,
                        &REGIMENES,
                    );
                    campo(
                        &mut cols[2],
                        "CP fiscal",
                        0.0,
                        &mut self.form_cliente.codigo_postal,
                        "91260",
                    );
                });
                ui.add_space(4.0);
                ui.columns(2, |cols| {
                    combo_clave(
                        &mut cols[0],
                        "Uso de CFDI",
                        "uso_cli",
                        &mut self.form_cliente.uso_cfdi,
                        &USOS_CFDI,
                    );
                    campo(
                        &mut cols[1],
                        "Correo",
                        0.0,
                        &mut self.form_cliente.email,
                        "cliente@correo.com",
                    );
                });
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Con RFC, régimen y CP, sus ventas se facturan aparte, no en la global \
                     del día.",
                    )
                    .size(12.0)
                    .color(theme::texto_suave()),
                );
                ui.add_space(12.0);
                if ui.add(theme::boton_acento("Guardar")).clicked() {
                    self.guardar_cliente();
                }
            });
        });
        ui.add_space(8.0);

        // --- abonos ---
        theme::marco_tarjeta(ui, |ui| {
            ui.label(
                egui::RichText::new("Abonar a la deuda")
                    .strong()
                    .color(theme::acento()),
            );
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Cliente")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                    let texto = self
                        .clientes
                        .iter()
                        .find(|c| Some(c.id) == self.cliente_abono)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| "Selecciona…".to_string());
                    egui::ComboBox::from_id_salt("abono")
                        .selected_text(texto)
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            let clientes = self.clientes.clone();
                            for c in clientes {
                                let etiqueta = format!(
                                    "{} (debe {})",
                                    c.name,
                                    Money::from_cents(c.saldo_cents).format_mxn()
                                );
                                if ui
                                    .selectable_label(self.cliente_abono == Some(c.id), etiqueta)
                                    .clicked()
                                {
                                    self.cliente_abono = Some(c.id);
                                }
                            }
                        });
                });
                campo(ui, "Monto", 110.0, &mut self.abono_monto, "0.00");
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(" ");
                    if ui.add(theme::boton_acento("Registrar abono")).clicked() {
                        self.abonar();
                    }
                });
            });
            if con_deuda > 0 {
                let aviso = if con_deuda == 1 {
                    "1 cliente tiene saldo pendiente.".to_string()
                } else {
                    format!("{con_deuda} clientes tienen saldo pendiente.")
                };
                ui.label(
                    egui::RichText::new(aviso)
                        .size(12.0)
                        .color(theme::texto_suave()),
                );
            }
        });
        ui.add_space(8.0);

        // --- la cartera ---
        ui.add_space(4.0);
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Cartera", Some(Icono::Persona));
            campo(
                ui,
                "Buscar",
                0.0,
                &mut self.filtro_clientes,
                "nombre, RFC o teléfono",
            );
            let q = self.filtro_clientes.to_lowercase();
            let mut clientes: Vec<_> = self
                .clientes
                .iter()
                .filter(|c| {
                    q.is_empty()
                        || c.name.to_lowercase().contains(&q)
                        || c.rfc.to_lowercase().contains(&q)
                        || c.phone.contains(&q)
                })
                .cloned()
                .collect();
            clientes.sort_by_key(|a| std::cmp::Reverse(a.saldo_cents));
            egui::ScrollArea::vertical()
                .id_salt("clientes")
                .show(ui, |ui| {
                    egui::Grid::new("clientes")
                        .num_columns(4)
                        .spacing([18.0, 10.0])
                        .min_row_height(40.0)
                        .striped(true)
                        .show(ui, |ui| {
                            for encabezado in ["Nombre", "Teléfono", "Debe", "Disponible"] {
                                ui.label(
                                    egui::RichText::new(encabezado)
                                        .strong()
                                        .size(12.0)
                                        .color(theme::texto_suave()),
                                );
                            }
                            ui.end_row();
                            for c in clientes {
                                ui.horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(
                                        egui::vec2(26.0, 26.0),
                                        egui::Sense::hover(),
                                    );
                                    theme::pintar_iniciales(
                                        ui.painter(),
                                        r,
                                        &c.name,
                                        theme::acento(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(&c.name);
                                });
                                ui.label(&c.phone);
                                if c.saldo_cents > 0 {
                                    ui.colored_label(
                                        theme::peligro(),
                                        Money::from_cents(c.saldo_cents).format_mxn(),
                                    );
                                } else {
                                    ui.label(Money::ZERO.format_mxn());
                                }
                                ui.label(Money::from_cents(c.disponible_cents).format_mxn());
                                ui.end_row();
                            }
                        });
                });
        });
    }

    fn guardar_cliente(&mut self) {
        let limite = Money::parse(&self.form_cliente_limite).unwrap_or(Money::ZERO);
        let cliente = NuevoClienteDto {
            name: self.form_cliente.name.trim().to_string(),
            rfc: self.form_cliente.rfc.trim().to_uppercase(),
            phone: self.form_cliente.phone.trim().to_string(),
            email: self.form_cliente.email.trim().to_string(),
            credit_limit_cents: limite.cents(),
            regimen_fiscal: self.form_cliente.regimen_fiscal.trim().to_string(),
            codigo_postal: self.form_cliente.codigo_postal.trim().to_string(),
            uso_cfdi: self.form_cliente.uso_cfdi.trim().to_uppercase(),
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::GuardarCliente { cliente },
        }) {
            pos_api::Respuesta::Ok(_) => {
                self.mensaje = "✓ Cliente guardado. Ya puedes fiarle desde la venta (F2).".into();
                self.form_cliente = NuevoClienteDto::default();
                self.form_cliente_limite.clear();
                self.mostrar_alta_cliente = false;
                self.refrescar_clientes();
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
        }
    }

    fn abonar(&mut self) {
        let Some(id) = self.cliente_abono else {
            self.mensaje = "Selecciona un cliente".into();
            return;
        };
        let monto = Money::parse(&self.abono_monto).unwrap_or(Money::ZERO);
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::Abonar {
                cliente_id: id,
                monto_cents: monto.cents(),
                metodo: "efectivo".to_string(),
            },
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Texto(saldo)) => {
                self.mensaje = format!("✓ Abono registrado. Saldo actual: {saldo}");
                self.abono_monto.clear();
                self.refrescar_clientes();
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn pantalla_catalogo(&mut self, ui: &mut egui::Ui) {
        theme::titulo_pantalla(
            ui,
            "Catálogo e inventario",
            "Da de alta en 30 segundos. Lo de aquí aparece en la venta (F2).",
            Icono::CajaProducto,
        );

        // ------------------------------------------- alta rápida (hero) ----
        // Lo primero y lo más grande: 3 campos, lenguaje de tienda, sin SKU
        // ni SAT. El modo avanzado (SKU, categoría, SAT) queda abajo plegado.
        self.tarjeta_alta_rapida(ui);
        ui.add_space(8.0);

        if self.puede_pedidos() {
            theme::marco_tarjeta(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    Icono::Camion.grande(ui, 28.0, theme::acento());
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Pedidos a proveedor")
                                .strong()
                                .size(16.0),
                        );
                        ui.label(
                            egui::RichText::new(if self.bajo_minimo.is_empty() {
                                "Pides, llega y el anaquel sube.".into()
                            } else {
                                format!(
                                    "{} producto(s) bajo el mínimo: pídelos al proveedor.",
                                    self.bajo_minimo.len()
                                )
                            })
                            .size(12.0)
                            .color(if self.bajo_minimo.is_empty() {
                                theme::texto_suave()
                            } else {
                                theme::advertencia()
                            }),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(theme::boton_acento("Abrir pedidos")).clicked() {
                            self.ir_a(Pantalla::Pedidos);
                        }
                    });
                });
            });
            ui.add_space(8.0);
        }

        // --------------------------------------- avanzado / edición ----
        //
        // El alta normal es la tarjeta de arriba (30 segundos). Esto es el
        // modo avanzado: SKU a mano, categoría y SAT. Plegado de entrada.
        let editando = self.editando_id.is_some();
        egui::CollapsingHeader::new(
            egui::RichText::new(if editando {
                "Editando producto"
            } else {
                "Modo avanzado: SKU, categoría y SAT (opcional)"
            })
            .strong()
            .size(15.0),
        )
        .id_salt("alta_producto")
        .open(if editando { Some(true) } else { None })
        .default_open(false)
        .show(ui, |ui| {
        theme::marco_tarjeta(ui, |ui| {
            ui.horizontal(|ui| {
                theme::seccion(
                    ui,
                    if editando {
                        "Editando producto"
                    } else {
                        "Producto nuevo"
                    },
                    Some(Icono::CajaProducto),
                );
                if editando {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Cancelar").clicked() {
                            self.editando_id = None;
                            self.form = FormProducto::default();
                        }
                    });
                }
            });
            ui.add_space(8.0);

            theme::seccion(ui, "Qué es", Some(Icono::Etiqueta));
            ui.add_space(4.0);
            ui.columns(3, |cols| {
                campo(&mut cols[0], "SKU", 0.0, &mut self.form.sku, "7501");
                campo(
                    &mut cols[1],
                    "Nombre",
                    0.0,
                    &mut self.form.nombre,
                    "Coca-Cola 600 ml",
                );
                campo(
                    &mut cols[2],
                    "Código de barras",
                    0.0,
                    &mut self.form.codigo,
                    "",
                );
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Código propio")
                    .on_hover_text("EAN-13 interno (prefijo 20) para granel, ropa y lo que llega sin código")
                    .clicked()
                {
                    self.asignar_codigo_propio();
                }
                if ui
                    .button("Imprimir código")
                    .on_hover_text("Etiqueta en la térmica: nombre, precio y barras")
                    .clicked()
                {
                    self.imprimir_codigo_propio();
                }
                ui.label(
                    egui::RichText::new("El lector lo cobra igual que uno de fábrica. Guarda el producto.")
                        .size(12.0)
                        .color(theme::texto_suave()),
                );
            });
            ui.add_space(8.0);

            theme::seccion(ui, "A cuánto", Some(Icono::Billete));
            ui.add_space(4.0);
            ui.columns(3, |cols| {
                campo(&mut cols[0], "Precio", 0.0, &mut self.form.precio, "18.00");
                campo(&mut cols[1], "Costo", 0.0, &mut self.form.costo, "9.00");
                if !editando {
                    campo(
                        &mut cols[2],
                        "Stock inicial",
                        0.0,
                        &mut self.form.stock,
                        "24",
                    );
                }
            });
            ui.add_space(8.0);

            egui::CollapsingHeader::new(
                egui::RichText::new("Para el SAT (opcional)")
                    .strong()
                    .size(14.0),
            )
            .id_salt("sat_producto")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.columns(3, |cols| {
                    campo(
                        &mut cols[0],
                        "Clave",
                        0.0,
                        &mut self.form.clave_sat,
                        "01010101",
                    );
                    campo(
                        &mut cols[1],
                        "Unidad",
                        0.0,
                        &mut self.form.clave_unidad,
                        "H87",
                    );
                    campo(
                        &mut cols[2],
                        "Nombre de la unidad",
                        0.0,
                        &mut self.form.unidad_sat,
                        "Pieza",
                    );
                });
            });
            ui.add_space(12.0);

            ui.horizontal_wrapped(|ui| {
                let etiqueta = if editando {
                    "Guardar cambios"
                } else {
                    "Dar de alta"
                };
                if ui.add(theme::boton_acento(etiqueta)).clicked() {
                    self.guardar_producto();
                }
                if editando {
                    ui.label(
                        egui::RichText::new(
                            "El stock no se edita aquí: usa Entradas, ajustes y mermas.",
                        )
                        .size(12.0)
                        .color(theme::texto_suave()),
                    );
                }
            });
        });
        });
        ui.add_space(8.0);

        // La lista va aquí: es lo que se busca al abrir Catálogo. El alta
        // queda arriba; importar y ajustes, abajo.
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Tus productos", Some(Icono::CajaProducto));
            ui.label(
                egui::RichText::new("✏️ edita precio o nombre · 📥 mueve el stock abajo · 🗑️ da de baja sin borrar ventas.")
                    .size(12.0)
                    .color(theme::texto_suave()),
            );
            campo(
                ui,
                "Buscar en la lista",
                0.0,
                &mut self.filtro_catalogo,
                "nombre o código",
            );
            let q = self.filtro_catalogo.to_lowercase();
            let productos: Vec<_> = self
                .resultados
                .iter()
                .filter(|p| {
                    q.is_empty()
                        || p.sku.to_lowercase().contains(&q)
                        || p.name.to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
            let ctx = ui.ctx().clone();
            for p in &productos {
                self.asegurar_foto(&ctx, &p.sku);
            }
            // La tabla usa el scroll de la página. Antes tenía uno propio y
            // al bajar quedaban dos barras y la cabecera se perdía dentro
            // del recuadro: ahora hay un solo desplazamiento, predecible.
            egui::Grid::new("catalogo_grid")
                .num_columns(8)
                .spacing([14.0, 8.0])
                .min_row_height(36.0)
                .striped(true)
                .show(ui, |ui| {
                    for encabezado in
                        ["Código", "Nombre", "Precio", "Costo", "Hay", "Editar", "Stock", ""]
                    {
                        ui.label(
                            egui::RichText::new(encabezado)
                                .strong()
                                .color(theme::texto_suave())
                                .size(12.0),
                        );
                    }
                    ui.end_row();
                    for p in &productos {
                        ui.label(&p.sku);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(
                                egui::vec2(32.0, 32.0),
                                egui::Sense::hover(),
                            );
                            if let Some(tex) = self.fotos.get(&p.sku) {
                                ui.put(
                                    r,
                                    egui::Image::new(tex)
                                        .fit_to_exact_size(egui::vec2(32.0, 32.0))
                                        .corner_radius(egui::CornerRadius::same(8)),
                                );
                            } else {
                                theme::pintar_foto_producto(ui.painter(), r, &p.name);
                            }
                            ui.add_space(6.0);
                            ui.label(&p.name);
                        });
                        ui.label(Money::from_cents(p.price_cents).format_mxn());
                        ui.label(Money::from_cents(p.cost_cents).format_mxn());
                        let stock = if p.track_inventory {
                            Quantity::from_millis(p.stock_millis).format()
                        } else {
                            "n/a".to_string()
                        };
                        let agotado = p.track_inventory && p.stock_millis <= 0;
                        let bajo = p.track_inventory
                            && p.stock_millis > 0
                            && (p.stock_millis <= p.min_stock_millis
                                || p.stock_millis <= 1_000);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(stock).color(if agotado {
                                    theme::peligro()
                                } else if bajo {
                                    theme::advertencia()
                                } else {
                                    theme::texto()
                                }),
                            );
                            if agotado {
                                theme::pastilla(ui, "agotado", theme::peligro());
                            } else if bajo {
                                theme::pastilla(ui, "bajo mínimo", theme::advertencia());
                            }
                        });
                        if Icono::Lapiz
                            .boton(ui, 34.0, theme::acento(), "Editar nombre o precio")
                            .clicked()
                        {
                            self.cargar_para_editar(p);
                        }
                        if Icono::Entrada
                            .boton(ui, 34.0, theme::texto_suave(), "Entrada o merma: elige y baja a registrar")
                            .clicked()
                        {
                            self.inv_producto = Some(p.id);
                            self.inv_cantidad.clear();
                            self.inv_motivo.clear();
                            self.mensaje = format!(
                                "Ajustando {}: escribe cuántos abajo en «¿Entró o se mermó?»",
                                p.name
                            );
                        }
                        if !self.es_cajero()
                            && Icono::Basura
                                .boton(ui, 34.0, theme::peligro(), "Dar de baja (no borra ventas)")
                                .clicked()
                        {
                            let id = p.id;
                            self.desactivar(id);
                        }
                        ui.end_row();
                    }
                });
        });
        ui.add_space(8.0);

        // ------------------------------------------------------- importar CSV
        if self.es_admin() {
            theme::marco_tarjeta(ui, |ui| {
                theme::seccion(
                    ui,
                    "Traer desde Excel (opcional)",
                    Some(Icono::CajaProducto),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Si ya tienes tu lista en Excel, guárdala como .csv y tráela. \
                     El programa adivina las columnas (nombre, precio, existencia…). \
                     Si viene rara, «Elegir columnas» te deja decir qué es cada una.",
                    )
                    .size(12.0)
                    .color(theme::texto_suave()),
                );
                ui.add_space(8.0);
                campo(
                    ui,
                    "Archivo",
                    0.0,
                    &mut self.ruta_csv,
                    "arrastra el archivo aquí o escribe la ruta",
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.add(theme::boton_acento("Importar")).clicked() {
                        self.importar_csv();
                    }
                    if ui
                        .add(theme::boton_suave("Elegir columnas…"))
                        .on_hover_text("Abre el archivo y te deja decir qué columna es cuál")
                        .clicked()
                    {
                        self.analizar_csv();
                    }
                    ui.checkbox(&mut self.csv_actualizar, "Actualizar existentes");
                });
                if let Some(r) = &self.ultimo_importe {
                    ui.separator();
                    let (texto, color) = if r.errores.is_empty() {
                        (
                            format!(
                                "Listo: {} creados, {} actualizados de {} renglones.",
                                r.creados, r.actualizados, r.renglones
                            ),
                            theme::acento(),
                        )
                    } else {
                        (
                            format!(
                            "{} renglones con error. No se importó nada hasta que los corrijas.",
                            r.errores.len()
                        ),
                            theme::peligro(),
                        )
                    };
                    ui.colored_label(color, texto);
                    if !r.columnas_reconocidas.is_empty() {
                        ui.label(
                            egui::RichText::new(format!(
                                "Columnas reconocidas: {}",
                                r.columnas_reconocidas.join(", ")
                            ))
                            .color(theme::texto_suave()),
                        );
                    }
                    if !r.errores.is_empty() {
                        egui::ScrollArea::vertical()
                            .id_salt("errores_csv")
                            .max_height(140.0)
                            .show(ui, |ui| {
                                egui::Grid::new("grid_errores")
                                    .num_columns(3)
                                    .spacing([10.0, 3.0])
                                    .show(ui, |ui| {
                                        for e in r.errores.iter().take(200) {
                                            ui.label(format!("renglón {}", e.renglon));
                                            ui.label(
                                                e.columna
                                                    .clone()
                                                    .unwrap_or_else(|| "—".to_string()),
                                            );
                                            ui.colored_label(theme::peligro(), &e.motivo);
                                            ui.end_row();
                                        }
                                    });
                            });
                    }
                }
            });

            ui.add_space(8.0);
            theme::marco_tarjeta(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(theme::boton_suave("Exportar catálogo a CSV"))
                        .clicked()
                    {
                        self.exportar_csv();
                    }
                    ui.label(
                        egui::RichText::new("se guarda como catalogo.csv junto al programa")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                });
            });
        }
        ui.add_space(8.0);

        // ------------------------------------- entradas y mermas ----
        // Antes: cantidad con signo + motivo libre. Ahora: stepper, motivo
        // con botones (Compra / Merma / Ajuste) y campo libre sólo si se
        // quiere detallar. El signo lo ponen los botones Entrada/Merma.
        if !self.es_cajero() {
            theme::marco_tarjeta(ui, |ui| {
                theme::seccion(
                    ui,
                    "¿Entró o se mermó mercancía?",
                    Some(Icono::CajaProducto),
                );
                ui.label(
                    egui::RichText::new("1. Elige el producto · 2. Cuántos · 3. Entrada o merma.")
                        .size(12.0)
                        .color(theme::texto_suave()),
                );
                ui.add_space(8.0);
                ui.columns(2, |cols| {
                    cols[0].vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Producto")
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                        let texto = self
                            .resultados
                            .iter()
                            .find(|p| Some(p.id) == self.inv_producto)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| {
                                "Toca un producto de la lista o elígeme…".to_string()
                            });
                        egui::ComboBox::from_id_salt("inv_producto")
                            .selected_text(texto)
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                let lista = self.resultados.clone();
                                for p in lista {
                                    if ui
                                        .selectable_label(self.inv_producto == Some(p.id), &p.name)
                                        .clicked()
                                    {
                                        self.inv_producto = Some(p.id);
                                    }
                                }
                            });
                    });
                    cols[1].vertical(|ui| {
                        ui.label(
                            egui::RichText::new("¿Cuántos?")
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                        ui.horizontal(|ui| {
                            if ui.add_sized([40.0, 40.0], egui::Button::new("−")).clicked() {
                                let actual =
                                    Quantity::parse(&self.inv_cantidad).unwrap_or(Quantity::ZERO);
                                let menos = (actual.millis() - 1_000).max(0);
                                self.inv_cantidad = if menos == 0 {
                                    String::new()
                                } else {
                                    Quantity::from_millis(menos).format()
                                };
                            }
                            ui.add_sized(
                                [ui.available_width() - 88.0, 40.0],
                                egui::TextEdit::singleline(&mut self.inv_cantidad)
                                    .hint_text("Ej.: 12")
                                    .font(egui::FontId::proportional(18.0)),
                            );
                            if ui.add_sized([40.0, 40.0], egui::Button::new("+")).clicked() {
                                let actual =
                                    Quantity::parse(&self.inv_cantidad).unwrap_or(Quantity::ZERO);
                                self.inv_cantidad =
                                    Quantity::from_millis(actual.millis() + 1_000).format();
                            }
                        });
                    });
                });
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Motivo (toca uno)")
                        .size(12.0)
                        .color(theme::texto_suave()),
                );
                ui.horizontal_wrapped(|ui| {
                    for motivo in ["Compra al proveedor", "Merma", "Ajuste", "Devolución"] {
                        let puesto = self.inv_motivo.trim() == motivo
                            || (self.inv_motivo.trim().is_empty()
                                && motivo == "Compra al proveedor");
                        if ui
                            .selectable_label(puesto, motivo)
                            .on_hover_text(match motivo {
                                "Compra al proveedor" => "Llegó mercancía: suma",
                                "Merma" => "Se echó a perder o se rompió: resta",
                                "Ajuste" => "Corregir el conteo",
                                _ => "Un cliente devolvió",
                            })
                            .clicked()
                        {
                            self.inv_motivo = motivo.to_string();
                        }
                    }
                    ui.add_sized(
                        [220.0, 32.0],
                        egui::TextEdit::singleline(&mut self.inv_motivo)
                            .hint_text("o escribe el detalle…"),
                    );
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_sized([200.0, 46.0], theme::boton_acento("✓ Entrada (suma)"))
                        .on_hover_text("Suma lo que llegó al stock")
                        .clicked()
                    {
                        // El signo lo pone el botón: el comerciante escribe "12",
                        // no "+12" ni "-12".
                        self.inv_cantidad = self
                            .inv_cantidad
                            .trim()
                            .trim_start_matches('+')
                            .trim_start_matches('-')
                            .to_string();
                        if self.inv_motivo.trim().is_empty() {
                            self.inv_motivo = "Compra al proveedor".to_string();
                        }
                        self.ajustar_inventario();
                    }
                    if ui
                        .add_sized([200.0, 46.0], theme::boton_peligro("− Merma (resta)"))
                        .on_hover_text("Resta lo que se perdió del stock")
                        .clicked()
                    {
                        let q = Quantity::parse(&self.inv_cantidad).unwrap_or(Quantity::ZERO);
                        if q.millis() > 0 {
                            self.inv_cantidad = format!("-{}", q.format());
                        }
                        if self.inv_motivo.trim().is_empty()
                            || self.inv_motivo.trim() == "Compra al proveedor"
                        {
                            self.inv_motivo = "Merma".to_string();
                        }
                        self.ajustar_inventario();
                    }
                });
            });
        }
    }

    /// Tarjeta hero del alta rápida: lo primero de Catálogo.
    ///
    /// Tres pasos en lenguaje de tienda (no "SKU", no "mapeo"):
    /// 1. ¿Qué vendes?  2. ¿A cuánto?  3. ¿Cuántos tienes?
    ///
    ///    El costo y el código son opcionales y van en el segundo renglón.
    fn tarjeta_alta_rapida(&mut self, ui: &mut egui::Ui) {
        theme::marco_tarjeta(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(r.center(), 20.0, theme::tenir(theme::acento(), 0.16));
                Icono::Mas.dibujar(ui.painter(), r.center(), 22.0, theme::acento());
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Nuevo producto — en 30 segundos")
                            .strong()
                            .size(18.0),
                    );
                    ui.label(
                        egui::RichText::new(
                            "1. ¿Qué vendes?  2. ¿A cuánto?  3. ¿Cuántos tienes? Y listo.",
                        )
                        .size(12.5)
                        .color(theme::texto_suave()),
                    );
                });
            });
            ui.add_space(10.0);

            // Renglón 1: lo obligatorio. El nombre llena; precio y stock fijos.
            // Enter en cualquiera de los tres guarda (si hay nombre y precio):
            // dar de alta 20 productos no debe pedir el ratón.
            let mut foco_en_alta = false;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("¿Qué vendes? *")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                    let r = ui.add_sized(
                        [ui.available_width(), 46.0],
                        egui::TextEdit::singleline(&mut self.alta_nombre)
                            .hint_text("Ej.: Coca 600 ml, Pan dulce, Jabón…")
                            .font(egui::FontId::proportional(19.0)),
                    );
                    if self.foco_alta {
                        r.request_focus();
                    }
                    foco_en_alta |= r.has_focus();
                });
            });
            ui.add_space(6.0);
            ui.columns(3, |cols| {
                cols[0].vertical(|ui| {
                    ui.label(
                        egui::RichText::new("¿A cuánto? *")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                    let r = ui.add_sized(
                        [ui.available_width(), 46.0],
                        egui::TextEdit::singleline(&mut self.alta_precio)
                            .hint_text("Ej.: 18.50")
                            .font(egui::FontId::proportional(20.0)),
                    );
                    foco_en_alta |= r.has_focus();
                });
                cols[1].vertical(|ui| {
                    ui.label(
                        egui::RichText::new("¿Cuántos tienes?")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized([40.0, 46.0], egui::Button::new("−"))
                            .on_hover_text("Uno menos")
                            .clicked()
                        {
                            let actual =
                                Quantity::parse(&self.alta_stock).unwrap_or(Quantity::ZERO);
                            let menos = (actual.millis() - 1_000).max(0);
                            self.alta_stock = if menos == 0 {
                                String::new()
                            } else {
                                Quantity::from_millis(menos).format()
                            };
                        }
                        let r = ui.add_sized(
                            [ui.available_width() - 88.0, 46.0],
                            egui::TextEdit::singleline(&mut self.alta_stock)
                                .hint_text("Ej.: 24")
                                .font(egui::FontId::proportional(20.0)),
                        );
                        foco_en_alta |= r.has_focus();
                        if ui
                            .add_sized([40.0, 46.0], egui::Button::new("+"))
                            .on_hover_text("Uno más")
                            .clicked()
                        {
                            let actual =
                                Quantity::parse(&self.alta_stock).unwrap_or(Quantity::ZERO);
                            let mas = actual.millis() + 1_000;
                            self.alta_stock = Quantity::from_millis(mas).format();
                        }
                    });
                });
                cols[2].vertical(|ui| {
                    ui.label(
                        egui::RichText::new("¿Cuánto te costó? (opcional)")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                    ui.add_sized(
                        [ui.available_width(), 46.0],
                        egui::TextEdit::singleline(&mut self.alta_costo)
                            .hint_text("Para saber tu ganancia")
                            .font(egui::FontId::proportional(18.0)),
                    );
                });
            });
            ui.add_space(6.0);
            // Renglón 2: lo opcional + guardar.
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Código de barras (si trae, si no se genera solo)")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                    ui.add_sized(
                        [200.0, 40.0],
                        egui::TextEdit::singleline(&mut self.alta_codigo)
                            .hint_text("Escanea o déjalo vacío"),
                    );
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.add_space(18.0);
                    if ui
                        .add_sized([220.0, 46.0], theme::boton_acento("Guardar producto"))
                        .clicked()
                    {
                        self.guardar_alta_rapida();
                    }
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.add_space(18.0);
                    // Vista previa de la ganancia: lo que le importa al bolsillo.
                    if let (Some(p), Some(c)) = (
                        Money::parse(&self.alta_precio),
                        Money::parse(&self.alta_costo),
                    ) {
                        if p.cents() > 0 && c.cents() > 0 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Ganas {} por pieza",
                                    Money::from_cents(p.cents() - c.cents()).format_mxn()
                                ))
                                .size(13.0)
                                .color(theme::exito())
                                .strong(),
                            );
                        }
                    }
                });
            });
            if !self.alta_mensaje.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(&self.alta_mensaje)
                        .size(13.5)
                        .strong()
                        .color(if self.alta_ok {
                            theme::exito()
                        } else {
                            theme::peligro()
                        }),
                );
            }
            // Enter guarda sin ratón, sólo si el cursor está en los tres
            // campos de arriba (no en el filtro de la lista ni en el CSV).
            // El foco inicial se consume una sola vez aquí.
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            self.foco_alta = false;
            if enter
                && foco_en_alta
                && !self.alta_nombre.trim().is_empty()
                && !self.alta_precio.trim().is_empty()
            {
                self.guardar_alta_rapida();
            }
        });
    }

    /// Trae la lista de categorías (se usa en el combo del catálogo).
    fn refrescar_categorias(&mut self) {
        if let Cuerpo::Categorias(c) = self.ejecutar(Comando::Categorias) {
            self.categorias = c;
        }
    }

    fn cargar_para_editar(&mut self, p: &ProductoDto) {
        self.editando_id = Some(p.id);
        let codigo = match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::CodigosDe { producto_id: p.id },
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Texto(c)) => c,
            _ => String::new(),
        };
        self.form = FormProducto {
            sku: p.sku.clone(),
            nombre: p.name.clone(),
            precio: format!("{:.2}", p.price_cents as f64 / 100.0),
            costo: format!("{:.2}", p.cost_cents as f64 / 100.0),
            codigo,
            stock: String::new(),
            categoria: String::new(),
            clave_sat: p.clave_sat.clone(),
            clave_unidad: p.clave_unidad.clone(),
            unidad_sat: p.unidad_sat.clone(),
        };
        self.mensaje = format!("Editando {}", p.name);
        self.mostrar_alta_producto = true;
    }

    fn asignar_codigo_propio(&mut self) {
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::CodigoPropio,
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Texto(c)) => {
                self.form.codigo = c.clone();
                self.mensaje =
                    format!("Código propio {c}. Guárdalo para que el lector lo encuentre.");
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "No pude asignar un código propio.".into(),
        }
    }

    fn imprimir_codigo_propio(&mut self) {
        let codigo = self.form.codigo.trim().to_string();
        if codigo.is_empty() {
            self.asignar_codigo_propio();
        }
        let codigo = self.form.codigo.trim().to_string();
        let nombre = if self.form.nombre.trim().is_empty() {
            "Producto".to_string()
        } else {
            self.form.nombre.trim().to_string()
        };
        let precio_cents = Money::parse(&self.form.precio)
            .unwrap_or(Money::ZERO)
            .cents();
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::ImprimirEtiqueta {
                codigo,
                nombre,
                precio_cents,
            },
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Texto(t)) => self.mensaje = t,
            pos_api::Respuesta::Error(e) => self.mensaje = e,
            _ => self.mensaje = "No pude imprimir la etiqueta.".into(),
        }
    }

    fn guardar_producto(&mut self) {
        let precio = Money::parse(&self.form.precio).unwrap_or(Money::ZERO);
        let costo = Money::parse(&self.form.costo).unwrap_or(Money::ZERO);
        let stock = Quantity::parse(&self.form.stock).unwrap_or(Quantity::ZERO);
        let producto = pos_api::NuevoProductoDto {
            id: self.editando_id,
            sku: self.form.sku.trim().to_string(),
            name: self.form.nombre.trim().to_string(),
            price_cents: precio.cents(),
            cost_cents: costo.cents(),
            tax_bp: 1600,
            unit: "pza".to_string(),
            stock_millis: stock.millis(),
            min_stock_millis: 0,
            track_inventory: true,
            barcodes: vec![self.form.codigo.trim().to_string()],
            categoria: if self.categoria_nueva.trim().is_empty() {
                let c = self.form.categoria.trim();
                if c.is_empty() {
                    None
                } else {
                    Some(c.to_string())
                }
            } else {
                Some(self.categoria_nueva.trim().to_string())
            },
            clave_sat: Some(self.form.clave_sat.clone()),
            clave_unidad: Some(self.form.clave_unidad.clone()),
            unidad_sat: Some(self.form.unidad_sat.clone()),
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::GuardarProducto { producto },
        }) {
            pos_api::Respuesta::Ok(_) => {
                self.mensaje = if self.editando_id.is_some() {
                    "✓ Producto actualizado. Ya se vende con el cambio.".to_string()
                } else {
                    "✓ Producto guardado. Ya se vende en F2.".to_string()
                };
                self.form = FormProducto::default();
                self.editando_id = None;
                self.categoria_nueva.clear();
                self.buscar("");
                self.refrescar_categorias();
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
        }
    }

    /// Alta rápida en 3 pasos: nombre, precio y cuántos hay.
    ///
    /// Para el comerciante común: no pide SKU, ni SAT, ni categoría, ni IVA.
    /// El SKU se genera solo (`P<milis>`), el IVA es 16 %, la unidad es pieza
    /// y las claves SAT son las genéricas. Si escribió código de barras, se
    /// guarda; si no, el producto se vende buscándolo por nombre.
    fn guardar_alta_rapida(&mut self) {
        let nombre = self.alta_nombre.trim().to_string();
        if nombre.is_empty() {
            self.alta_mensaje = "Escribe qué vendes. Ej.: Coca 600 ml".into();
            self.alta_ok = false;
            return;
        }
        let Some(precio) = Money::parse(&self.alta_precio) else {
            self.alta_mensaje = "¿A cuánto lo vendes? Ej.: 18.50".into();
            self.alta_ok = false;
            return;
        };
        if precio.cents() <= 0 {
            self.alta_mensaje = "El precio debe ser mayor a $0.00".into();
            self.alta_ok = false;
            return;
        }
        let stock = if self.alta_stock.trim().is_empty() {
            Quantity::ZERO
        } else {
            match Quantity::parse(&self.alta_stock) {
                Some(q) if q.millis() >= 0 => q,
                _ => {
                    self.alta_mensaje = "¿Cuántos tienes? Sólo números. Ej.: 24".into();
                    self.alta_ok = false;
                    return;
                }
            }
        };
        let costo = Money::parse(&self.alta_costo).unwrap_or(Money::ZERO);
        let codigo = self.alta_codigo.trim().to_string();
        // SKU: el que escribió o uno propio. Con milisegundos no choca ni
        // dando dos altas seguidas.
        let sku = if codigo.is_empty() {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            format!("P{ms}")
        } else {
            codigo.clone()
        };
        let producto = pos_api::NuevoProductoDto {
            id: None,
            sku: sku.clone(),
            name: nombre.clone(),
            price_cents: precio.cents(),
            cost_cents: costo.cents().max(0),
            tax_bp: 1600,
            unit: "pza".to_string(),
            stock_millis: stock.millis(),
            min_stock_millis: 5_000,
            track_inventory: true,
            barcodes: if codigo.is_empty() {
                vec![]
            } else {
                vec![codigo.clone()]
            },
            categoria: None,
            clave_sat: None,
            clave_unidad: None,
            unidad_sat: None,
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::GuardarProducto { producto },
        }) {
            pos_api::Respuesta::Ok(_) => {
                let ganancia = if costo.cents() > 0 {
                    format!(
                        " · ganas {} por pieza",
                        Money::from_cents(precio.cents() - costo.cents()).format_mxn()
                    )
                } else {
                    String::new()
                };
                self.alta_mensaje = format!("✓ {nombre} a {precio}{ganancia}. Ya se vende en F2.");
                self.alta_ok = true;
                self.alta_nombre.clear();
                self.alta_precio.clear();
                self.alta_stock.clear();
                self.alta_costo.clear();
                self.alta_codigo.clear();
                self.mensaje.clear();
                self.buscar("");
                self.refrescar_alertas();
            }
            pos_api::Respuesta::Error(e) => {
                // SKU duplicado (dos altas en el mismo milisegundo o código
                // repetido): se reintenta con sufijo en vez de culpar al usuario.
                if e.to_lowercase().contains("unique")
                    || e.to_lowercase().contains("duplic")
                    || e.to_lowercase().contains("sku")
                {
                    let ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let producto = pos_api::NuevoProductoDto {
                        id: None,
                        sku: format!("{sku}-{ms}"),
                        name: nombre.clone(),
                        price_cents: precio.cents(),
                        cost_cents: costo.cents().max(0),
                        tax_bp: 1600,
                        unit: "pza".to_string(),
                        stock_millis: stock.millis(),
                        min_stock_millis: 5_000,
                        track_inventory: true,
                        barcodes: if codigo.is_empty() {
                            vec![]
                        } else {
                            vec![codigo]
                        },
                        categoria: None,
                        clave_sat: None,
                        clave_unidad: None,
                        unidad_sat: None,
                    };
                    match self.api.ejecutar(Peticion {
                        token: self.token(),
                        comando: Comando::GuardarProducto { producto },
                    }) {
                        pos_api::Respuesta::Ok(_) => {
                            self.alta_mensaje =
                                format!("✓ {nombre} a {precio}. Ya se vende en F2.");
                            self.alta_ok = true;
                            self.alta_nombre.clear();
                            self.alta_precio.clear();
                            self.alta_stock.clear();
                            self.alta_costo.clear();
                            self.alta_codigo.clear();
                            self.buscar("");
                            self.refrescar_alertas();
                            return;
                        }
                        pos_api::Respuesta::Error(e2) => {
                            self.alta_mensaje = e2;
                            self.alta_ok = false;
                            return;
                        }
                    }
                }
                self.alta_mensaje = e;
                self.alta_ok = false;
            }
        }
    }

    fn desactivar(&mut self, id: i64) {
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::DesactivarProducto { id },
        }) {
            pos_api::Respuesta::Ok(_) => {
                self.mensaje =
                    "Producto dado de baja (las ventas anteriores siguen en los reportes)"
                        .to_string();
                self.buscar("");
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
        }
    }

    fn ajustar_inventario(&mut self) {
        let Some(id) = self.inv_producto else {
            self.mensaje = "Selecciona un producto".into();
            return;
        };
        let cantidad = Quantity::parse(&self.inv_cantidad).unwrap_or(Quantity::ZERO);
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::AjusteInventario {
                producto_id: id,
                delta_millis: cantidad.millis(),
                motivo: self.inv_motivo.clone(),
            },
        }) {
            pos_api::Respuesta::Ok(_) => {
                self.mensaje = "✓ Movimiento guardado. El anaquel ya cambió.".to_string();
                self.inv_cantidad.clear();
                self.inv_motivo.clear();
                self.buscar("");
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
        }
    }

    fn importar_csv(&mut self) {
        let ruta = self.ruta_csv.trim().trim_matches('"').to_string();
        if ruta.is_empty() {
            self.mensaje = "Escribe la ruta del archivo o arrastralo a la ventana".into();
            return;
        }
        let texto = match std::fs::read_to_string(&ruta) {
            Ok(t) => t,
            Err(e) => {
                self.mensaje = format!("No pude leer {ruta}: {e}");
                return;
            }
        };
        let Some(token) = self.token.clone() else {
            return;
        };
        match self.api.importar_csv(&token, &texto, self.csv_actualizar) {
            Ok(r) => {
                self.mensaje = if r.errores.is_empty() {
                    format!(
                        "Importación lista: {} creados y {} actualizados",
                        r.creados, r.actualizados
                    )
                } else {
                    format!(
                        "{} de {} renglones no pasaron. Corrige el archivo y vuelve a intentar.",
                        r.errores.len(),
                        r.renglones + r.errores.len()
                    )
                };
                self.ultimo_importe = Some(r);
                self.buscar("");
            }
            Err(e) => self.mensaje = e,
        }
    }

    fn exportar_csv(&mut self) {
        let Some(token) = self.token.clone() else {
            return;
        };
        match self.api.exportar_csv(&token) {
            Ok(csv) => {
                let ruta = std::env::current_dir()
                    .unwrap_or_default()
                    .join("catalogo.csv");
                match std::fs::write(&ruta, csv.as_bytes()) {
                    Ok(()) => {
                        let lineas = csv.lines().count().saturating_sub(1);
                        self.mensaje =
                            format!("Se exportaron {lineas} productos a {}", ruta.display());
                    }
                    Err(e) => self.mensaje = format!("No pude escribir el archivo: {e}"),
                }
            }
            Err(e) => self.mensaje = e,
        }
    }

    /// Arrastrar un CSV a la ventana lo deja listo para importar.
    fn revisar_archivos_arrastrados(&mut self, ctx: &egui::Context) {
        let archivos: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .map(|p| p.display().to_string())
                .collect()
        });
        if let Some(primero) = archivos.first() {
            self.ruta_csv = primero.clone();
            self.pantalla = Pantalla::Catalogo;
            if self.resultados.is_empty() {
                self.buscar("");
            }
            self.mensaje = format!("Archivo listo: {primero}. Dale a Importar.");
        }
    }

    // ------------------------------------------- mapeo de columnas (CSV) --

    /// Lee el archivo y abre el diálogo para decir qué columna es cuál.
    fn analizar_csv(&mut self) {
        let ruta = self.ruta_csv.trim().trim_matches('"').to_string();
        if ruta.is_empty() {
            self.mensaje = "Escribe la ruta del archivo o arrastralo a la ventana".into();
            return;
        }
        let texto = match std::fs::read_to_string(&ruta) {
            Ok(t) => t,
            Err(e) => {
                self.mensaje = format!("No pude leer {ruta}: {e}");
                return;
            }
        };
        if let Cuerpo::AnalisisCsv(a) = self.ejecutar(Comando::AnalizarCsv { texto }) {
            if a.encabezados.is_empty() {
                self.mensaje = "El archivo está vacío o no tiene encabezados".to_string();
                return;
            }
            // Lo que el sistema adivinó es el punto de partida: el cajero
            // sólo corrige lo que esté mal.
            self.mapeo = a.sugerido.clone();
            self.analisis = Some(a);
            self.mostrar_mapeo = true;
        }
    }

    fn importar_con_mapeo(&mut self) {
        let ruta = self.ruta_csv.trim().trim_matches('"').to_string();
        let texto = match std::fs::read_to_string(&ruta) {
            Ok(t) => t,
            Err(e) => {
                self.mensaje = format!("No pude leer {ruta}: {e}");
                return;
            }
        };
        let mapeo = self.mapeo.clone();
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::ImportarCsv {
                texto,
                actualizar: self.csv_actualizar,
                mapeo,
            },
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Importe(r)) => {
                self.mensaje = if r.errores.is_empty() {
                    format!(
                        "Importación lista: {} creados y {} actualizados",
                        r.creados, r.actualizados
                    )
                } else {
                    format!(
                        "{} renglones no pasaron. Corrige el archivo y vuelve a intentar.",
                        r.errores.len()
                    )
                };
                self.ultimo_importe = Some(r);
                self.mostrar_mapeo = false;
                self.analisis = None;
                self.buscar("");
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    /// El diálogo: una lista por columna, con el ejemplo del archivo al lado.
    fn ventana_mapeo(&mut self, ctx: &egui::Context) {
        let mut abierta = self.mostrar_mapeo;
        let Some(analisis) = self.analisis.clone() else {
            self.mostrar_mapeo = false;
            return;
        };
        let campos = analisis.campos.clone();
        let ejemplo = analisis.ejemplo.clone();

        egui::Window::new("¿Qué columna es cuál?")
            .open(&mut abierta)
            .collapsible(false)
            .resizable(true)
            .default_size([760.0, 520.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "El sistema ya adivinó. Si alguna columna quedó mal, cámbiala aquí.",
                    )
                    .heading(),
                );
                ui.separator();

                if !analisis.tiene_nombre {
                    ui.colored_label(
                        theme::peligro(),
                        "Falta la columna del nombre: sin ella no se puede dar de alta nada. \
                         Dile a cuál columna corresponde el nombre.",
                    );
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} columnas · {} renglones · separador '{}'",
                        analisis.encabezados.len(),
                        analisis.renglones,
                        analisis.separador
                    ))
                    .color(theme::texto_suave()),
                );
                ui.add_space(6.0);

                egui::ScrollArea::vertical()
                    .id_salt("mapeo")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("grid_mapeo")
                            .num_columns(3)
                            .striped(true)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Columna del archivo").strong());
                                ui.label(egui::RichText::new("Es").strong());
                                ui.label(egui::RichText::new("Ejemplo").strong());
                                ui.end_row();

                                for (i, encabezado) in analisis.encabezados.iter().enumerate() {
                                    ui.label(egui::RichText::new(encabezado).monospace());
                                    if self.mapeo.len() <= i {
                                        self.mapeo.resize(i + 1, "ignorar".to_string());
                                    }
                                    let elegido = self.mapeo[i].clone();
                                    let etiqueta = campos
                                        .iter()
                                        .find(|(c, _)| *c == elegido)
                                        .map(|(_, e)| e.to_string())
                                        .unwrap_or_else(|| elegido.clone());
                                    egui::ComboBox::from_id_salt(("mapeo", i))
                                        .width(240.0)
                                        .selected_text(etiqueta)
                                        .show_ui(ui, |ui| {
                                            for (clave, etiqueta) in &campos {
                                                ui.selectable_value(
                                                    &mut self.mapeo[i],
                                                    clave.to_string(),
                                                    etiqueta.clone(),
                                                );
                                            }
                                        });
                                    let muestra = ejemplo
                                        .iter()
                                        .map(|fila| fila.get(i).cloned().unwrap_or_default())
                                        .collect::<Vec<_>>()
                                        .join(" · ");
                                    ui.label(egui::RichText::new(muestra).monospace().weak());
                                    ui.end_row();
                                }
                            });
                    });

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Importar así")
                                    .color(theme::tinta_sobre(theme::acento())),
                            )
                            .fill(theme::acento()),
                        )
                        .clicked()
                    {
                        self.importar_con_mapeo();
                    }
                    if ui.button("Volver a adivinar").clicked() {
                        self.mapeo = analisis.sugerido.clone();
                    }
                    if ui.button("Cancelar").clicked() {
                        self.mostrar_mapeo = false;
                        self.analisis = None;
                    }
                    ui.checkbox(&mut self.csv_actualizar, "Actualizar existentes");
                });
            });
        if !abierta {
            self.mostrar_mapeo = false;
            self.analisis = None;
        }
    }

    /// Ventas de hoy.
    ///
    /// Todo cabe en una sola columna que se recorre: primero los números del
    /// día, luego el detalle. Nada se corta por falta de alto.
    fn pantalla_reportes(&mut self, ui: &mut egui::Ui) {
        theme::titulo_pantalla(
            ui,
            "Ventas de hoy",
            "Cómo va el día: cuánto se vendió, qué dejó de margen y quién vendió qué.",
            Icono::Grafica,
        );

        let totales = match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::TotalesHoy,
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Totales(t)) => t,
            _ => Default::default(),
        };

        // --- cuatro números que importan; el resto, plegado ---
        // El ticket promedio dice si se vende mucho de a poco o poco de a
        // mucho: con 10 tickets de $500 se atiende distinto que con 100 de $50.
        let promedio_cents = if totales.count > 0 {
            totales.total_cents / totales.count
        } else {
            0
        };
        ui.columns(4, |cols| {
            tarjeta_dato(
                &mut cols[0],
                "Tickets",
                &totales.count.to_string(),
                theme::texto(),
                Icono::Ticket,
            );
            tarjeta_dato(
                &mut cols[1],
                "Vendido",
                &Money::from_cents(totales.total_cents).format_mxn(),
                theme::acento(),
                Icono::Grafica,
            );
            tarjeta_dato(
                &mut cols[2],
                "Ticket promedio",
                &Money::from_cents(promedio_cents).format_mxn(),
                theme::texto(),
                Icono::Billete,
            );
            tarjeta_dato(
                &mut cols[3],
                "Utilidad",
                &Money::from_cents(totales.utilidad_cents).format_mxn(),
                if totales.utilidad_cents >= 0 {
                    theme::exito()
                } else {
                    theme::peligro()
                },
                Icono::Grafica,
            );
        });
        ui.add_space(6.0);
        egui::CollapsingHeader::new(egui::RichText::new("Desglose del día").strong().size(14.0))
            .id_salt("desglose_dia")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.columns(4, |cols| {
                    tarjeta_dato(
                        &mut cols[0],
                        "IVA",
                        &Money::from_cents(totales.tax_cents).format_mxn(),
                        theme::texto(),
                        Icono::Factura,
                    );
                    tarjeta_dato(
                        &mut cols[1],
                        "Descuentos",
                        &Money::from_cents(totales.discount_cents).format_mxn(),
                        theme::peligro(),
                        Icono::Etiqueta,
                    );
                    tarjeta_dato(
                        &mut cols[2],
                        "Costo",
                        &Money::from_cents(totales.costo_cents).format_mxn(),
                        theme::texto(),
                        Icono::CajaProducto,
                    );
                    tarjeta_dato(
                        &mut cols[3],
                        "Efectivo",
                        &Money::from_cents(totales.efectivo_cents).format_mxn(),
                        theme::texto(),
                        Icono::Billete,
                    );
                });
                ui.add_space(6.0);
                ui.columns(3, |cols| {
                    tarjeta_dato(
                        &mut cols[0],
                        "Tarjeta",
                        &Money::from_cents(totales.tarjeta_cents).format_mxn(),
                        theme::texto(),
                        Icono::Tarjeta,
                    );
                    tarjeta_dato(
                        &mut cols[1],
                        "Transferencia",
                        &Money::from_cents(totales.transferencia_cents).format_mxn(),
                        theme::texto(),
                        Icono::Tienda,
                    );
                    tarjeta_dato(
                        &mut cols[2],
                        "Otros",
                        &Money::from_cents(totales.otros_cents).format_mxn(),
                        theme::texto(),
                        Icono::Foco,
                    );
                });
            });
        ui.add_space(8.0);

        // --- ventas por cajero ---
        theme::marco_tarjeta(ui, |ui| {
            ui.horizontal(|ui| {
                theme::seccion(ui, "Ventas por cajero", Some(Icono::Persona));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(theme::boton_suave("Actualizar")).clicked() {
                        self.refrescar_cajeros();
                    }
                });
            });
            ui.add_space(4.0);
            let cajeros = self.cajeros.clone();
            egui::Grid::new("cajeros")
                .num_columns(5)
                .spacing([18.0, 10.0])
                .min_row_height(36.0)
                .striped(true)
                .show(ui, |ui| {
                    for encabezado in ["Cajero", "Tickets", "Vendido", "Devuelto", ""] {
                        ui.label(
                            egui::RichText::new(encabezado)
                                .strong()
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                    }
                    ui.end_row();
                    // La barra compara contra el que más vendió: dos números
                    // se leen, doscientos no.
                    let maximo = cajeros
                        .iter()
                        .map(|c| c.vendido_cents)
                        .max()
                        .unwrap_or(0)
                        .max(1);
                    for c in &cajeros {
                        ui.horizontal(|ui| {
                            let (r, _) = ui
                                .allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                            theme::pintar_iniciales(ui.painter(), r, &c.nombre, theme::acento());
                            ui.add_space(5.0);
                            ui.label(&c.nombre);
                        });
                        ui.label(c.tickets.to_string());
                        ui.label(Money::from_cents(c.vendido_cents).format_mxn());
                        if c.devuelto_cents > 0 {
                            ui.colored_label(
                                theme::peligro(),
                                Money::from_cents(c.devuelto_cents).format_mxn(),
                            );
                        } else {
                            ui.label(Money::ZERO.format_mxn());
                        }
                        barra(
                            ui,
                            120.0,
                            c.vendido_cents as f32 / maximo as f32,
                            theme::acento(),
                        );
                        ui.end_row();
                    }
                });
        });
        ui.add_space(8.0);

        // --- quién debe: el dueño lo mira aquí, no en clientes ---
        let deudores: Vec<_> = self
            .clientes
            .iter()
            .filter(|c| c.saldo_cents > 0)
            .cloned()
            .collect();
        if !deudores.is_empty() {
            theme::marco_tarjeta(ui, |ui| {
                theme::seccion(ui, "Quién debe", Some(Icono::Persona));
                ui.add_space(4.0);
                let maximo = deudores
                    .iter()
                    .map(|c| c.saldo_cents)
                    .max()
                    .unwrap_or(0)
                    .max(1);
                egui::Grid::new("deudores")
                    .num_columns(3)
                    .spacing([18.0, 10.0])
                    .min_row_height(36.0)
                    .striped(true)
                    .show(ui, |ui| {
                        for encabezado in ["Cliente", "Debe", ""] {
                            ui.label(
                                egui::RichText::new(encabezado)
                                    .strong()
                                    .size(12.0)
                                    .color(theme::texto_suave()),
                            );
                        }
                        ui.end_row();
                        for c in &deudores {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(24.0, 24.0),
                                    egui::Sense::hover(),
                                );
                                theme::pintar_iniciales(ui.painter(), r, &c.name, theme::acento());
                                ui.add_space(5.0);
                                ui.label(&c.name);
                            });
                            ui.colored_label(
                                theme::peligro(),
                                Money::from_cents(c.saldo_cents).format_mxn(),
                            );
                            barra(
                                ui,
                                160.0,
                                c.saldo_cents as f32 / maximo as f32,
                                theme::peligro(),
                            );
                            ui.end_row();
                        }
                    });
            });
            ui.add_space(8.0);
        }

        // --- lo que más sale, con su barra para comparar de un golpe ---
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Productos más vendidos (30 días)", Some(Icono::Grafica));
            ui.add_space(4.0);
            let top = match self.api.ejecutar(Peticion {
                token: self.token(),
                comando: Comando::TopProductos { limite: 10 },
            }) {
                pos_api::Respuesta::Ok(Cuerpo::Top(t)) => t,
                _ => Vec::new(),
            };
            // La barra más larga es la del que más se vendió: las demás son
            // proporcionales a esa.
            let maximo = top
                .iter()
                .map(|t| t.cantidad_millis)
                .max()
                .unwrap_or(0)
                .max(1);
            egui::Grid::new("top")
                .num_columns(4)
                .spacing([18.0, 6.0])
                .min_row_height(26.0)
                .show(ui, |ui| {
                    for encabezado in ["Producto", "Cantidad", "Importe", ""] {
                        ui.label(
                            egui::RichText::new(encabezado)
                                .strong()
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                    }
                    ui.end_row();
                    for t in top {
                        ui.label(&t.nombre);
                        ui.label(Quantity::from_millis(t.cantidad_millis).format());
                        ui.label(Money::from_cents(t.importe_cents).format_mxn());
                        // Barra de comparación: 140 px para el primero.
                        let fraccion = t.cantidad_millis.max(0) as f32 / maximo as f32;
                        barra(ui, 140.0, fraccion, theme::acento());
                        ui.end_row();
                    }
                });
        });
        ui.add_space(8.0);

        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Utilidad por producto (hoy)", Some(Icono::Billete));
            ui.label(
                egui::RichText::new(
                    "El costo es el de cuando se vendió, no el de ahora. Si cambiaste el costo, el margen de esas ventas no se mueve.",
                )
                .size(12.0)
                .color(theme::texto_suave()),
            );
            ui.add_space(4.0);
            let utilidad = match self.api.ejecutar(Peticion {
                token: self.token(),
                comando: Comando::UtilidadHoy { limite: 12 },
            }) {
                pos_api::Respuesta::Ok(Cuerpo::Utilidad(t)) => t,
                _ => Vec::new(),
            };
            if utilidad.is_empty() {
                ui.colored_label(theme::texto_suave(), "Hoy todavía no hay ventas con costo.");
            } else {
                let maximo = utilidad
                    .iter()
                    .map(|u| u.utilidad_cents.abs())
                    .max()
                    .unwrap_or(0)
                    .max(1);
                egui::Grid::new("utilidad_hoy")
                    .num_columns(5)
                    .spacing([18.0, 6.0])
                    .min_row_height(26.0)
                    .striped(true)
                    .show(ui, |ui| {
                        for encabezado in ["Producto", "Vendido", "Costo", "Utilidad", ""] {
                            ui.label(
                                egui::RichText::new(encabezado)
                                    .strong()
                                    .size(12.0)
                                    .color(theme::texto_suave()),
                            );
                        }
                        ui.end_row();
                        for u in utilidad {
                            ui.label(&u.nombre);
                            ui.label(Money::from_cents(u.vendido_cents).format_mxn());
                            ui.label(Money::from_cents(u.costo_cents).format_mxn());
                            let color = if u.utilidad_cents >= 0 {
                                theme::exito()
                            } else {
                                theme::peligro()
                            };
                            ui.colored_label(
                                color,
                                Money::from_cents(u.utilidad_cents).format_mxn(),
                            );
                            let fraccion = u.utilidad_cents.abs() as f32 / maximo as f32;
                            barra(ui, 140.0, fraccion, color);
                            ui.end_row();
                        }
                    });
            }
        });
        ui.add_space(8.0);

        // --- sacar el periodo a un archivo y reimprimir ---
        theme::marco_tarjeta(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Exportar ventas").strong());
                ui.label("del");
                ui.add_sized(
                    [110.0, 34.0],
                    egui::TextEdit::singleline(&mut self.desde).hint_text("hoy"),
                );
                ui.label("al");
                ui.add_sized(
                    [110.0, 34.0],
                    egui::TextEdit::singleline(&mut self.hasta).hint_text("hoy"),
                );
                if ui.button("Exportar a CSV").clicked() {
                    self.exportar_ventas();
                }
                ui.label(
                    egui::RichText::new("formato AAAA-MM-DD; vacío = hoy")
                        .size(12.0)
                        .color(theme::texto_suave()),
                );
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Reimprimir un ticket").strong());
                ui.add_sized(
                    [120.0, 34.0],
                    egui::TextEdit::singleline(&mut self.folio_buscar).hint_text("000001"),
                );
                if ui.button("Buscar e imprimir").clicked() {
                    self.reimprimir(self.folio_buscar.trim().to_string());
                }
            });
        });
        ui.add_space(8.0);

        // --- lo último que se vendió ---
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(
                ui,
                "Ultimas ventas (devolver o reimprimir)",
                Some(Icono::Ticket),
            );
            ui.add_space(4.0);
            let recientes = match self.api.ejecutar(Peticion {
                token: self.token(),
                comando: Comando::VentasRecientes { limite: 12 },
            }) {
                pos_api::Respuesta::Ok(Cuerpo::Ventas(v)) => v,
                _ => Vec::new(),
            };
            egui::Grid::new("recientes")
                .num_columns(6)
                .spacing([14.0, 6.0])
                .min_row_height(28.0)
                .striped(true)
                .show(ui, |ui| {
                    for encabezado in ["Folio", "Fecha", "Total", "Estado", "", ""] {
                        ui.label(
                            egui::RichText::new(encabezado)
                                .strong()
                                .size(12.0)
                                .color(theme::texto_suave()),
                        );
                    }
                    ui.end_row();
                    for v in recientes {
                        ui.label(&v.folio);
                        ui.label(&v.created_at);
                        ui.label(Money::from_cents(v.total_cents).format_mxn());
                        let (texto_estado, color_estado) = match v.status.as_str() {
                            "completada" => ("Completada", theme::exito()),
                            "cancelada" => ("Cancelada", theme::peligro()),
                            "devuelta" => ("Devuelta", theme::advertencia()),
                            otro => (otro, theme::texto_suave()),
                        };
                        theme::pastilla(ui, texto_estado, color_estado);
                        if ui.small_button("reimprimir").clicked() {
                            let folio = v.folio.clone();
                            self.reimprimir(folio);
                        }
                        if v.status == "completada" {
                            if ui.small_button("devolver").clicked() {
                                let folio = v.folio.clone();
                                self.devolver(folio);
                            }
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
        });
    }

    /// Pide el estado de los respaldos y las unidades donde se pueden guardar.
    fn refrescar_respaldos(&mut self) {
        if let Cuerpo::Respaldos(e) = self.ejecutar(Comando::Respaldos) {
            // Si el usuario ya escribió una carpeta a mano, se respeta.
            if self.ruta_respaldo.trim().is_empty() {
                self.ruta_respaldo = e.carpeta.clone();
            }
            self.respaldos = e;
        }
        if let Cuerpo::Unidades(u) = self.ejecutar(Comando::Unidades) {
            self.unidades = u;
        }
    }

    /// Pide la bitácora con los filtros actuales.
    fn refrescar_auditoria(&mut self) {
        let comando = Comando::Auditoria {
            desde: self.aud_desde.clone(),
            hasta: self.aud_hasta.clone(),
            usuario: self.aud_usuario.clone(),
            accion: self.aud_accion.clone(),
            texto: self.aud_texto.clone(),
            limite: 300,
        };
        if let Cuerpo::Auditoria(a) = self.ejecutar(comando) {
            self.bitacora = a;
        }
    }

    fn respaldar_ahora(&mut self) {
        let carpeta = self.ruta_respaldo.trim().to_string();
        let comando = if carpeta.is_empty() {
            Comando::RespaldarAhora { carpeta: None }
        } else {
            Comando::RespaldarAhora {
                carpeta: Some(carpeta),
            }
        };
        if let Cuerpo::Respaldos(e) = self.ejecutar(comando) {
            self.mensaje = e.mensaje.clone();
            self.respaldos = e;
        }
    }

    fn verificar_respaldos(&mut self) {
        if let pos_api::Respuesta::Ok(Cuerpo::Texto(t)) = self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::VerificarRespaldos,
        }) {
            self.mensaje = t;
        } else if let pos_api::Respuesta::Error(e) = self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::VerificarRespaldos,
        }) {
            self.mensaje = e;
        }
    }

    fn refrescar_cajeros(&mut self) {
        if let Cuerpo::Cajeros(c) = self.ejecutar(Comando::TotalesPorCajero) {
            self.cajeros = c;
        }
    }

    fn exportar_ventas(&mut self) {
        let r = self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::ExportarVentasCsv {
                desde: self.desde.clone(),
                hasta: self.hasta.clone(),
            },
        });
        match r {
            pos_api::Respuesta::Ok(Cuerpo::Texto(csv)) => {
                let nombre = if self.desde.trim().is_empty() {
                    "ventas-hoy.csv".to_string()
                } else {
                    format!("ventas-{}-a-{}.csv", self.desde.trim(), self.hasta.trim())
                };
                let ruta = std::env::current_dir().unwrap_or_default().join(nombre);
                match std::fs::write(&ruta, csv.as_bytes()) {
                    Ok(()) => {
                        let lineas = csv.lines().count().saturating_sub(1);
                        self.mensaje =
                            format!("Se exportaron {lineas} ventas a {}", ruta.display());
                    }
                    Err(e) => self.mensaje = format!("No pude escribir el archivo: {e}"),
                }
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    /// Vuelve a mandar el ticket a la impresora (el cliente lo perdió, o se
    /// atascó el papel). La venta no se toca: es sólo una reimpresión.
    fn reimprimir(&mut self, folio: String) {
        if folio.is_empty() {
            return;
        }
        let Some(token) = self.token.clone() else {
            return;
        };
        match self.api.ticket_por_folio(&token, &folio) {
            Ok(ticket) => {
                let datos = self.datos_ticket_guardado(&ticket);
                let bytes = datos.to_escpos_con(&self.perfil(), false);
                self.imprimir(bytes);
                self.mensaje = format!("Reimprimiendo el ticket {folio}");
            }
            Err(e) => self.mensaje = e,
        }
    }

    fn devolver(&mut self, folio: String) {
        let r = self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::Devolver {
                folio,
                supervisor: None,
            },
        });
        self.mensaje = match r {
            pos_api::Respuesta::Ok(_) => "Venta devuelta".to_string(),
            pos_api::Respuesta::Error(e) => e,
        };
    }

    // ------------------------------------------------------------ conteo --

    fn refrescar_conteo(&mut self) {
        if let Cuerpo::Conteo(c) = self.ejecutar(Comando::VerConteo { conteo_id: None }) {
            self.conteo = c;
        }
    }

    fn iniciar_conteo(&mut self) {
        let categoria = self.conteo_categoria.trim().to_string();
        let categoria = if categoria.is_empty() {
            None
        } else {
            Some(categoria)
        };
        if let Cuerpo::Conteo(c) = self.ejecutar(Comando::IniciarConteo { categoria }) {
            if c.id > 0 {
                self.conteo = c;
                self.conteo_codigo.clear();
                self.mensaje = format!(
                    "Conteo empezado: {} productos por contar. Escanea y captura lo que hay.",
                    self.conteo.productos
                );
            }
        }
    }

    /// Escanea (o escribe) un código y anota lo contado. Sin cantidad, cuenta 1.
    fn contar_escaneado(&mut self) {
        let codigo = self.conteo_codigo.trim().to_string();
        if codigo.is_empty() {
            return;
        }
        let millis = match self.conteo_cantidad.trim() {
            "" => 1_000,
            texto => match texto.replace(',', ".").parse::<f64>() {
                Ok(n) if n >= 0.0 => (n * 1000.0).round() as i64,
                _ => {
                    self.mensaje = format!("No entiendo la cantidad '{texto}'");
                    return;
                }
            },
        };
        // Lo que tenía antes, para avisar si se está recontando.
        let estaba = self
            .conteo
            .renglones
            .iter()
            .find(|r| r.sku == codigo)
            .and_then(|r| r.contado_millis);
        if let Cuerpo::Conteo(c) = self.ejecutar(Comando::ContarProducto {
            codigo: codigo.clone(),
            cantidad_millis: millis,
        }) {
            if c.id > 0 {
                self.conteo = c;
                self.conteo_codigo.clear();
                self.conteo_cantidad.clear();
                let nombre = self
                    .conteo
                    .renglones
                    .iter()
                    .find(|r| r.sku == codigo)
                    .map(|r| r.nombre.clone())
                    .unwrap_or_else(|| codigo.clone());
                self.mensaje = match estaba {
                    // Se volvió a contar: se reemplaza, no se suma.
                    Some(antes) => format!(
                        "{nombre}: {}. Se reemplazó lo anterior ({}) por lo de ahora.",
                        cantidad(millis),
                        cantidad(antes)
                    ),
                    None => format!(
                        "{nombre}: {}. Quedan {} por contar.",
                        cantidad(millis),
                        self.conteo.productos.saturating_sub(self.conteo.contados)
                    ),
                };
            }
        }
    }

    /// Cuenta un renglón desde la tabla, con la cantidad escrita.
    fn contar_renglon(&mut self, sku: &str) {
        let millis = match self.conteo_cantidad.trim() {
            "" => 1_000,
            texto => match texto.replace(',', ".").parse::<f64>() {
                Ok(n) if n >= 0.0 => (n * 1000.0).round() as i64,
                _ => {
                    self.mensaje = format!("No entiendo la cantidad '{texto}'");
                    return;
                }
            },
        };
        if let Cuerpo::Conteo(c) = self.ejecutar(Comando::ContarProducto {
            codigo: sku.to_string(),
            cantidad_millis: millis,
        }) {
            if c.id > 0 {
                self.conteo = c;
            }
        }
    }

    fn cancelar_conteo(&mut self) {
        let id = self.conteo.id;
        self.ejecutar(Comando::CancelarConteo { conteo_id: id });
        self.conteo = ConteoDto::default();
        self.conteo_codigo.clear();
        self.conteo_categoria.clear();
        self.mensaje = "Conteo cancelado. El inventario sigue igual.".to_string();
    }

    /// Cierra el conteo. Con `aplicar`, escribe los ajustes; sin él, sólo
    /// muestra el reporte.
    fn cerrar_conteo(&mut self, aplicar: bool) {
        let id = self.conteo.id;
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::CerrarConteo {
                conteo_id: id,
                aplicar,
                supervisor: None,
            },
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Texto(t)) => {
                self.mensaje = t.replace('\n', " · ");
                self.conteo = ConteoDto::default();
                self.mostrar_firma_conteo = false;
                self.buscar("");
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn pantalla_conteo(&mut self, ui: &mut egui::Ui) {
        theme::titulo_pantalla(
            ui,
            "Conteo físico",
            "Cuando el sistema dice 12 y en el anaquel hay 9, aquí se arregla: cuentas \
             escaneando, el sistema calcula la diferencia y queda anotado quién aplicó el ajuste.",
            Icono::Portapapeles,
        );

        // ------------------------------------------- sin conteo: empezar ----
        if self.conteo.id == 0 {
            theme::marco_tarjeta(ui, |ui| {
                ui.label(
                    egui::RichText::new("No hay ningún conteo abierto.")
                        .strong()
                        .size(16.0),
                );
                ui.label(
                    egui::RichText::new("Paso 1: elige la categoría (o todo). Paso 2: Empezar.")
                        .size(12.5)
                        .color(theme::texto_suave()),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Categoría");
                    let categorias = self.categorias.clone();
                    let elegida = self.conteo_categoria.clone();
                    let texto = if elegida.is_empty() {
                        "Todas (todo el inventario)".to_string()
                    } else {
                        elegida.clone()
                    };
                    egui::ComboBox::from_id_salt("conteo_categoria")
                        .selected_text(texto)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(elegida.is_empty(), "Todas (todo el inventario)")
                                .clicked()
                            {
                                self.conteo_categoria.clear();
                            }
                            for c in categorias {
                                let marcada = elegida == c.name;
                                if ui.selectable_label(marcada, &c.name).clicked() {
                                    self.conteo_categoria = c.name.clone();
                                }
                            }
                        });
                    if ui
                        .add_sized([180.0, 44.0], theme::boton_acento("Empezar conteo"))
                        .clicked()
                    {
                        self.iniciar_conteo();
                    }
                });
            });
            ui.label(
                egui::RichText::new(
                    "Se puede vender mientras se cuenta: la diferencia se calcula contra \
                     el stock de ese momento, no contra la foto del principio.",
                )
                .color(theme::texto_suave()),
            );
            return;
        }

        // ---------------------------------------------------- encabezado ----
        let c = self.conteo.clone();
        // Cuatro datos en columnas de verdad. Con `horizontal_wrapped` cada
        // tarjeta pedía el ancho completo y las otras tres se iban fuera de
        // la pantalla. En ventanas angostas se acomodan en dos filas.
        let datos: [(&str, String, egui::Color32, Icono); 4] = [
            (
                "Productos",
                c.productos.to_string(),
                theme::texto(),
                Icono::CajaProducto,
            ),
            (
                "Contados",
                format!("{} / {}", c.contados, c.productos),
                if c.contados >= c.productos {
                    theme::exito()
                } else {
                    theme::acento()
                },
                Icono::Paloma,
            ),
            (
                "Con diferencia",
                c.con_diferencia.to_string(),
                if c.con_diferencia > 0 {
                    theme::advertencia()
                } else {
                    theme::texto()
                },
                Icono::Alerta,
            ),
            (
                "Merma (a costo)",
                Money::from_cents(c.valor_merma_cents).format_mxn(),
                if c.valor_merma_cents > 0 {
                    theme::peligro()
                } else {
                    theme::texto()
                },
                Icono::Billete,
            ),
        ];
        let columnas = if ui.available_width() >= 900.0 { 4 } else { 2 };
        for (fila, grupo) in datos.chunks(columnas).enumerate() {
            if fila > 0 {
                ui.add_space(6.0);
            }
            ui.columns(columnas, |cols| {
                for (i, (etiqueta, valor, color, icono)) in grupo.iter().enumerate() {
                    tarjeta_dato(&mut cols[i], etiqueta, valor, *color, *icono);
                }
            });
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if c.filtro.is_empty() {
                    "Alcance: todo el inventario".to_string()
                } else {
                    format!("Alcance: {}", c.filtro)
                })
                .color(theme::texto_suave()),
            );
            ui.label(egui::RichText::new(format!("· Empezó {}", c.created_at)).weak());
        });
        ui.separator();

        // ------------------------------------------------------- escanear ----
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Código").strong());
            let respuesta = ui.add_sized(
                [240.0, 34.0],
                egui::TextEdit::singleline(&mut self.conteo_codigo)
                    .hint_text("escanea o escribe el SKU"),
            );
            ui.label("Cantidad");
            let cantidad_resp = ui.add_sized(
                [110.0, 34.0],
                egui::TextEdit::singleline(&mut self.conteo_cantidad).hint_text("1"),
            );
            if ui.button("Contar").clicked() {
                self.contar_escaneado();
            }
            // Enter en cualquiera de los dos campos cuenta: es lo más rápido
            // con el lector en la mano.
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter && (respuesta.lost_focus() || cantidad_resp.lost_focus()) {
                self.contar_escaneado();
            }
        });
        ui.label(
            egui::RichText::new(
                "Escaneas y escribes cuántas hay. Si lo vuelves a escanear, se queda el \
                 último valor: no se suma.",
            )
            .color(theme::texto_suave()),
        );
        ui.separator();

        // -------------------------------------------------- filtros y tabla ----
        let filtro = self.conteo_filtro.clone();
        let renglones: Vec<pos_api::RenglonConteoDto> = c
            .renglones
            .iter()
            .filter(|r| match filtro.as_str() {
                "por_contar" => r.contado_millis.is_none(),
                "diferencia" => r.diferencia_millis != Some(0) && r.diferencia_millis.is_some(),
                _ => true,
            })
            .cloned()
            .collect();

        theme::marco_tarjeta(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (clave, etiqueta) in [
                    ("todos", "Todos"),
                    ("por_contar", "Por contar"),
                    ("diferencia", "Con diferencia"),
                ] {
                    let marcado = self.conteo_filtro == clave;
                    if ui
                        .add(egui::SelectableLabel::new(marcado, etiqueta))
                        .clicked()
                    {
                        self.conteo_filtro = clave.to_string();
                    }
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} faltantes · {} sobrantes (unidades)",
                        cantidad(c.faltantes_millis),
                        cantidad(c.sobrantes_millis)
                    ))
                    .color(theme::texto_suave()),
                );
            });
            ui.add_space(6.0);

            // La tabla se desplaza con la página: antes tenía su propio
            // scroll y quedaban dos barras, una dentro de otra.
            egui::Grid::new("grid_conteo")
                .num_columns(6)
                .striped(true)
                .spacing([14.0, 5.0])
                .show(ui, |ui| {
                    for etiqueta in [
                        "Producto",
                        "Decía",
                        "Contaste",
                        "Hay ahora",
                        "Diferencia",
                        "",
                    ] {
                        ui.label(egui::RichText::new(etiqueta).strong());
                    }
                    ui.end_row();

                    for r in &renglones {
                        ui.label(&r.nombre);
                        ui.label(cantidad(r.esperado_millis));
                        match r.contado_millis {
                            Some(n) => {
                                ui.label(
                                    egui::RichText::new(cantidad(n))
                                        .strong()
                                        .color(theme::acento()),
                                );
                            }
                            None => {
                                ui.label(egui::RichText::new("—").color(theme::texto_suave()));
                            }
                        }
                        ui.label(cantidad(r.actual_millis));
                        // La diferencia en pastilla: es la columna que se
                        // busca con la mirada, un número suelto se pierde.
                        match r.diferencia_millis {
                            None => {
                                ui.label(
                                    egui::RichText::new("sin contar")
                                        .color(theme::texto_suave()),
                                );
                            }
                            Some(0) => {
                                theme::pastilla(ui, "cuadra", theme::acento());
                            }
                            Some(d) if d < 0 => {
                                theme::pastilla(ui, &cantidad(d), theme::peligro());
                            }
                            Some(d) => {
                                theme::pastilla(
                                    ui,
                                    &format!("+{}", cantidad(d)),
                                    theme::advertencia(),
                                );
                            }
                        }
                        if ui.button("Contar").clicked() {
                            self.contar_renglon(&r.sku);
                        }
                        ui.end_row();
                    }
                });
            if renglones.is_empty() {
                ui.colored_label(theme::acento(), "Nada que mostrar con ese filtro.");
            }
        });

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Cerrar y aplicar ajustes")
                            .color(theme::tinta_sobre(theme::acento())),
                    )
                    .fill(theme::acento()),
                )
                .on_hover_text(
                    "Escribe los ajustes en el inventario. Queda anotado quién lo aplicó.",
                )
                .clicked()
            {
                if self.conteo.contados == 0 {
                    self.mensaje = "No has contado nada todavía.".to_string();
                } else {
                    self.mostrar_firma_conteo = true;
                }
            }
            if ui
                .button("Cerrar sin aplicar")
                .on_hover_text("Ves el reporte, pero el inventario no se toca")
                .clicked()
            {
                self.cerrar_conteo(false);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Cancelar conteo")
                            .color(theme::tinta_sobre(theme::peligro())),
                    )
                    .fill(theme::peligro()),
                )
                .on_hover_text("Tira lo contado y deja el inventario como estaba")
                .clicked()
            {
                self.cancelar_conteo();
            }
        });
    }

    /// Confirmar antes de escribir los ajustes: mueve el inventario.
    fn ventana_firma_conteo(&mut self, ctx: &egui::Context) {
        let mut abierta = self.mostrar_firma_conteo;
        let resumen = self.conteo.clone();
        egui::Window::new("Aplicar el conteo")
            .open(&mut abierta)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(520.0);
                ui.label(
                    egui::RichText::new("Esto mueve el inventario. Queda anotado quién lo aplicó.")
                        .heading(),
                );
                ui.separator();
                fila(
                    ui,
                    "Productos contados",
                    &format!("{} de {}", resumen.contados, resumen.productos),
                    None,
                );
                fila(
                    ui,
                    "Con diferencia",
                    &resumen.con_diferencia.to_string(),
                    None,
                );
                fila(
                    ui,
                    "Faltantes",
                    &cantidad(resumen.faltantes_millis),
                    Some(theme::peligro()),
                );
                fila(
                    ui,
                    "Sobrantes",
                    &cantidad(resumen.sobrantes_millis),
                    Some(theme::advertencia()),
                );
                fila_monto(
                    ui,
                    "Merma valuada a costo",
                    resumen.valor_merma_cents,
                    theme::texto_suave(),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Aplicar").clicked() {
                        self.cerrar_conteo(true);
                    }
                    if ui.button("Cancelar").clicked() {
                        self.mostrar_firma_conteo = false;
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.cerrar_conteo(true);
                }
            });
        if !abierta {
            self.mostrar_firma_conteo = false;
        }
    }

    fn pantalla_auditoria(&mut self, ui: &mut egui::Ui) {
        theme::titulo_pantalla(
            ui,
            "Bitácora",
            "Quién hizo qué y cuándo. Nada de aquí se borra.",
            Icono::Libro,
        );

        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Filtros", Some(Icono::Buscar));
            ui.horizontal_wrapped(|ui| {
                ui.label("Desde");
                ui.add_sized(
                    [124.0, 26.0],
                    egui::TextEdit::singleline(&mut self.aud_desde).hint_text("AAAA-MM-DD"),
                );
                ui.label("Hasta");
                ui.add_sized(
                    [124.0, 26.0],
                    egui::TextEdit::singleline(&mut self.aud_hasta).hint_text("AAAA-MM-DD"),
                );
                ui.label("Usuario");
                ui.add_sized(
                    [110.0, 26.0],
                    egui::TextEdit::singleline(&mut self.aud_usuario).hint_text("todos"),
                );
                ui.label("Texto");
                ui.add_sized(
                    [140.0, 26.0],
                    egui::TextEdit::singleline(&mut self.aud_texto).hint_text("folio, precio..."),
                );
                if ui.button("Buscar").clicked() {
                    self.refrescar_auditoria();
                }
                if ui.button("Hoy").clicked() {
                    self.aud_desde = ahora_cdmx();
                    self.aud_hasta = ahora_cdmx();
                    self.refrescar_auditoria();
                }
                if ui.button("Limpiar").clicked() {
                    self.aud_desde.clear();
                    self.aud_hasta.clear();
                    self.aud_usuario.clear();
                    self.aud_accion.clear();
                    self.aud_texto.clear();
                    self.refrescar_auditoria();
                }
            });
        });

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Acción:").color(theme::texto_suave()));
            if ui
                .add(egui::SelectableLabel::new(
                    self.aud_accion.is_empty(),
                    "todas",
                ))
                .clicked()
            {
                self.aud_accion.clear();
                self.refrescar_auditoria();
            }
            for accion in self.bitacora.acciones.clone() {
                let marcada = self.aud_accion == accion;
                if ui
                    .add(egui::SelectableLabel::new(
                        marcada,
                        etiqueta_accion(&accion),
                    ))
                    .clicked()
                {
                    self.aud_accion = if marcada { String::new() } else { accion };
                    self.refrescar_auditoria();
                }
            }
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} de {} eventos",
                    self.bitacora.mostrados, self.bitacora.total
                ))
                .strong(),
            );
            // Atajo para lo que más se busca cuando falta dinero.
            let solo_autorizaciones = self.aud_accion == "autorizar";
            if ui
                .add(egui::SelectableLabel::new(
                    solo_autorizaciones,
                    "Solo autorizaciones",
                ))
                .on_hover_text("Descuentos, fiados y cortes que no cuadran")
                .clicked()
            {
                self.aud_accion = if solo_autorizaciones {
                    String::new()
                } else {
                    "autorizar".to_string()
                };
                self.refrescar_auditoria();
            }
            let solo_precios = self.aud_accion == "precio";
            if ui
                .add(egui::SelectableLabel::new(
                    solo_precios,
                    "Solo precios a mano",
                ))
                .on_hover_text("Quién cambió un precio en el ticket, y de cuánto a cuánto")
                .clicked()
            {
                self.aud_accion = if solo_precios {
                    String::new()
                } else {
                    "precio".to_string()
                };
                self.refrescar_auditoria();
            }
        });

        ui.separator();

        if self.bitacora.eventos.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                let (r, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                ui.painter().circle_filled(
                    r.center(),
                    24.0,
                    theme::tenir(theme::texto_suave(), 0.14),
                );
                Icono::Libro.dibujar(ui.painter(), r.center(), 26.0, theme::texto_suave());
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Nada por aquí con esos filtros.")
                        .color(theme::texto_suave()),
                );
            });
            return;
        }

        theme::marco_tarjeta(ui, |ui| {
            // La columna de "Qué pasó" se queda con el ancho que sobra. Sin
            // este límite, un detalle largo estira la tabla y se sale de la
            // ventana en vez de bajar de renglón.
            let ancho_detalle = (ui.available_width() - 380.0).max(240.0);
            // La bitácora se desplaza con la página. Antes tenía su propio
            // scroll y aparecían dos barras: una para los filtros y otra
            // para los eventos.
            egui::Grid::new("grid_bitacora")
                .num_columns(3)
                .striped(true)
                .spacing([16.0, 8.0])
                .min_row_height(28.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Cuándo")
                            .strong()
                            .color(theme::texto_suave()),
                    );
                    ui.label(
                        egui::RichText::new("Quién")
                            .strong()
                            .color(theme::texto_suave()),
                    );
                    ui.label(
                        egui::RichText::new("Qué pasó")
                            .strong()
                            .color(theme::texto_suave()),
                    );
                    ui.end_row();

                    for e in self.bitacora.eventos.clone() {
                        ui.label(egui::RichText::new(&e.cuando).monospace().weak());
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(
                                egui::vec2(22.0, 22.0),
                                egui::Sense::hover(),
                            );
                            theme::pintar_iniciales(ui.painter(), r, &e.usuario, theme::acento());
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(&e.usuario).strong());
                        });
                        // Icono y color según de qué se trata: así se
                        // encuentra una autorización entre doscientos
                        // renglones sin tener que leerlos todos.
                        let (icono, color) = icono_de_accion(&e.accion);
                        ui.vertical(|ui| {
                            ui.set_max_width(ancho_detalle);
                            ui.horizontal_wrapped(|ui| {
                                icono.grande(ui, 18.0, color);
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(&e.etiqueta).strong().color(
                                    if matches!(
                                        e.accion.as_str(),
                                        "autorizar"
                                            | "precio"
                                            | "descuento"
                                            | "fiado"
                                            | "diferencia"
                                            | "retiro"
                                    ) {
                                        theme::advertencia()
                                    } else {
                                        theme::texto()
                                    },
                                ));
                                ui.label(
                                    egui::RichText::new(&e.detalle).color(theme::texto_suave()),
                                );
                            });
                        });
                        ui.end_row();
                    }
                });
        });
    }

    // -------------------------------------------------- actualizaciones --

    // --------------------------------------------------------- impresora --

    /// Refresca la lista en silencio: no toca lo que ya está configurado.
    fn refrescar_impresoras(&mut self) {
        if let Cuerpo::Impresoras(lista) = self.ejecutar(Comando::Impresoras) {
            self.impresoras = lista;
        }
    }

    /// Si todavía no hay impresora y el equipo tiene exactamente una
    /// confirmada, la deja puesta. Un puerto serie no se adivina.
    fn proponer_impresora_si_falta(&mut self) {
        if !self.config.impresora.trim().is_empty() {
            return;
        }
        let confirmadas: Vec<_> = self
            .impresoras
            .iter()
            .filter(|i| i.es_impresora)
            .cloned()
            .collect();
        if confirmadas.len() != 1 {
            return;
        }
        self.aplicar_impresora(&confirmadas[0]);
        if self.persistir_config() {
            self.mensaje = format!(
                "Encontré {} al abrir Configuración. Dale a Imprimir prueba.",
                confirmadas[0].etiqueta
            );
        }
    }

    fn aplicar_impresora(&mut self, i: &ImpresoraDto) {
        self.config.impresora = i.ruta.clone();
        self.config.impresora_perfil = i.perfil.clone();
        self.config.impresora_baudios = i.baudios;
    }

    /// Guarda la configuración actual. No toca el mensaje si salió bien.
    fn persistir_config(&mut self) -> bool {
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::GuardarConfig {
                config: self.config.clone(),
            },
        }) {
            pos_api::Respuesta::Ok(_) => true,
            pos_api::Respuesta::Error(e) => {
                self.mensaje = e;
                false
            }
        }
    }

    /// Busca impresoras en el equipo. No adivina el modelo: sólo se toma
    /// sola una confirmada. El resto se elige de la lista y se prueba.
    fn detectar_impresoras(&mut self) {
        if let Cuerpo::Impresoras(lista) = self.ejecutar(Comando::Impresoras) {
            self.impresoras = lista.clone();
            if lista.is_empty() {
                self.mensaje = "No encontré ninguna impresora. Conéctala, enciéndela y vuelve a pulsar Detectar. O corre: pos --impresoras".into();
                return;
            }
            let confirmadas: Vec<_> = lista.iter().filter(|i| i.es_impresora).cloned().collect();
            match confirmadas.len() {
                1 => {
                    self.aplicar_impresora(&confirmadas[0]);
                    let etiqueta = confirmadas[0].etiqueta.clone();
                    if self.persistir_config() {
                        self.mensaje = format!(
                            "Encontré {etiqueta}. No adiviné el modelo: miré el equipo. Dale a Imprimir prueba."
                        );
                    }
                }
                0 => {
                    self.mensaje = format!(
                        "Encontré {} puerto(s) que podrían ser la impresora. Elige uno de la lista y dale a Imprimir prueba. No adivino el modelo.",
                        lista.len()
                    );
                }
                n => {
                    self.mensaje = format!(
                        "Encontré {n} impresoras. Elige una de la lista y dale a Imprimir prueba. No adivino cuál es."
                    );
                }
            }
        }
    }

    /// Manda el ticket de prueba: la única forma honesta de saber si quedó bien.
    fn probar_impresion(&mut self, abrir_cajon: bool) {
        if self.config.impresora.trim().is_empty() {
            self.mensaje = "Escribe la ruta de la impresora o pulsa Detectar".into();
            return;
        }
        let ruta = self.config.impresora.trim().to_string();
        let baudios = self.config.impresora_baudios;
        let perfil = self.config.impresora_perfil.clone();
        if let Cuerpo::Prueba(p) = self.ejecutar(Comando::ProbarImpresion {
            ruta,
            baudios,
            perfil,
            abrir_cajon,
        }) {
            if p.ok {
                // El API ya guardó ruta, perfil y baudios. Aquí se guarda
                // el resto (ancho, cajón) por si el encargado los tocó.
                let msg = p.mensaje;
                let _ = self.persistir_config();
                self.mensaje = msg;
            } else {
                self.mensaje = p.mensaje;
            }
        }
    }

    fn refrescar_actualizacion(&mut self) {
        if let Cuerpo::EstadoActualizacion(e) = self.ejecutar(Comando::EstadoActualizacion) {
            self.actualizacion = e;
        }
    }

    fn revisar_paquete(&mut self) {
        let ruta = self.ruta_paquete.trim().trim_matches('"').to_string();
        if ruta.is_empty() {
            self.mensaje = "Escribe la ruta del paquete o arrastralo a la ventana".into();
            return;
        }
        if let Cuerpo::Paquete(p) = self.ejecutar(Comando::RevisarActualizacion { ruta }) {
            self.mensaje = if p.valido {
                format!(
                    "Paquete válido: versión {}. Léelo y dale a Actualizar.",
                    p.version
                )
            } else {
                p.motivo.clone()
            };
            self.paquete = Some(p);
        }
    }

    fn aplicar_paquete(&mut self) {
        let Some(ruta) = self.paquete.as_ref().map(|p| p.ruta.clone()) else {
            return;
        };
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::AplicarActualizacion { ruta },
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Texto(t)) => {
                self.mensaje = t;
                self.paquete = None;
                self.ruta_paquete.clear();
                self.refrescar_actualizacion();
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn revertir_actualizacion(&mut self) {
        match self.api.ejecutar(Peticion {
            token: self.token(),
            comando: Comando::RevertirActualizacion,
        }) {
            pos_api::Respuesta::Ok(Cuerpo::Texto(t)) => {
                self.mensaje = t;
                self.refrescar_actualizacion();
            }
            pos_api::Respuesta::Error(e) => self.mensaje = e,
            _ => {}
        }
    }

    fn seccion_lan(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        theme::marco_tarjeta(ui, |ui| {
            ui.horizontal(|ui| {
                theme::seccion(ui, "Celular en la LAN", Some(Icono::Tienda));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::DragValue::new(&mut self.config.puerto_lan).range(1..=65535));
                    ui.label(
                        egui::RichText::new("Puerto")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                });
            });
            let puerto = if self.config.puerto_lan <= 0 {
                8080
            } else {
                self.config.puerto_lan
            };
            let mut ips = direcciones_lan();
            ordenar_ips_lan(&mut ips);
            ui.add_space(4.0);
            if ips.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "No encontré una red. Conecta el equipo al Wi-Fi o al cable \
                         y vuelve a abrir esta pantalla.",
                    )
                    .color(theme::advertencia()),
                );
            } else {
                let url = format!("http://{}:{puerto}", ips[0]);
                ui.label(
                    egui::RichText::new("Abre esta dirección en el teléfono:")
                        .size(12.0)
                        .color(theme::texto_suave()),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&url)
                            .size(16.0)
                            .strong()
                            .color(theme::acento()),
                    );
                    if ui
                        .add(egui::Button::new("Copiar").min_size(egui::vec2(76.0, 30.0)))
                        .on_hover_text("Copia la dirección para pegarla en el teléfono")
                        .clicked()
                    {
                        ui.ctx().copy_text(url.clone());
                        self.mensaje = format!("Dirección copiada: {url}");
                    }
                });
                if !ip_privada(&ips[0]) {
                    ui.label(
                        egui::RichText::new(
                            "Esa no parece una dirección de la red local. Revisa el Wi-Fi \
                             del equipo o prueba una de las otras.",
                        )
                        .size(12.0)
                        .color(theme::advertencia()),
                    );
                }
                let otras: Vec<&String> = ips.iter().skip(1).collect();
                if !otras.is_empty() {
                    egui::CollapsingHeader::new(format!("Otras direcciones ({})", otras.len()))
                        .id_salt("otras_direcciones")
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(
                                    "Si la de arriba no abre en el teléfono, prueba una de estas:",
                                )
                                .size(12.0)
                                .color(theme::texto_suave()),
                            );
                            for ip in otras {
                                let url = format!("http://{ip}:{puerto}");
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(&url)
                                            .size(13.0)
                                            .color(theme::texto_suave()),
                                    );
                                    if ui
                                        .add(
                                            egui::Button::new("Copiar")
                                                .min_size(egui::vec2(64.0, 26.0)),
                                        )
                                        .clicked()
                                    {
                                        ui.ctx().copy_text(url.clone());
                                        self.mensaje = format!("Dirección copiada: {url}");
                                    }
                                });
                            }
                        });
                }
            }
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Arranca con --servir y apunta la app del teléfono a esa dirección.",
                )
                .size(12.0)
                .color(theme::texto_suave()),
            );
        });
    }

    fn pantalla_config(&mut self, ui: &mut egui::Ui) {
        theme::titulo_pantalla(
            ui,
            "Configuración",
            "Cómo se llama tu negocio, dónde imprime y cada cuánto respalda.",
            Icono::Engranaje,
        );

        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "El negocio", Some(Icono::Tienda));
            campo_grande(
                ui,
                "Nombre del negocio",
                &mut self.config.tienda,
                "Abarrotes La Esperanza",
            );
            campo_grande(
                ui,
                "Domicilio",
                &mut self.config.direccion,
                "Av. Siempre Viva 123, Col. Centro",
            );
            campo_grande(
                ui,
                "Leyenda del ticket",
                &mut self.config.pie,
                "Gracias por su compra · vuelva pronto",
            );
        });

        self.seccion_lan(ui);
        self.seccion_usuarios(ui);

        ui.add_space(10.0);
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Impresora de tickets", Some(Icono::Impresora));
            ui.label(
                egui::RichText::new(if cfg!(windows) {
                    "No adivina el modelo: lista las impresoras de Windows y les manda ESC/POS en RAW. Pulsa Detectar, elige la tuya e Imprimir prueba."
                } else {
                    "No adivina el modelo: mira el equipo. Pulsa Detectar y luego Imprimir prueba. El instalador ya dejó los permisos (grupos lp y dialout)."
                })
                .color(theme::texto_suave()),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Puerto")
                            .size(12.0)
                            .color(theme::texto_suave()),
                    );
                    ui.add_sized(
                        [280.0, 40.0],
                        egui::TextEdit::singleline(&mut self.config.impresora).hint_text(
                            if cfg!(windows) {
                                "POS-80 (el nombre en Impresoras y escáneres)"
                            } else {
                                "/dev/usb/lp0 · /dev/ttyUSB0 · 192.168.1.50:9100"
                            },
                        ),
                    );
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(" ");
                    if ui.add(theme::boton_suave("Detectar")).clicked() {
                        self.detectar_impresoras();
                    }
                });
            });

            // Lo que se encontró, para que el encargado no tenga que adivinar.
            if !self.impresoras.is_empty() {
                ui.add_space(2.0);
                for i in self.impresoras.clone() {
                    ui.horizontal(|ui| {
                        let marca = if i.es_impresora { "●" } else { "○" };
                        let texto = format!("{marca} {}", i.etiqueta);
                        if ui
                            .add(egui::SelectableLabel::new(
                                self.config.impresora == i.ruta,
                                texto,
                            ))
                            .on_hover_text(&i.nota)
                            .clicked()
                        {
                            self.config.impresora = i.ruta.clone();
                            self.config.impresora_perfil = i.perfil.clone();
                            self.config.impresora_baudios = i.baudios;
                        }
                    });
                }
            }

            ui.horizontal(|ui| {
                ui.label("Modelo");
                let perfil_actual = self.config.impresora_perfil.clone();
                egui::ComboBox::from_id_salt("perfil_impresora")
                    .selected_text(perfil_actual.clone())
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for nombre in pos_hw::Perfil::nombres() {
                            ui.selectable_value(
                                &mut self.config.impresora_perfil,
                                nombre.clone(),
                                nombre,
                            );
                        }
                    });
                ui.label(
                    egui::RichText::new(
                        pos_hw::Perfil::por_nombre(&perfil_actual)
                            .unwrap_or_default()
                            .pagina
                            .nota(),
                    )
                    .color(theme::texto_suave()),
                );
            });

            // Los baudios sólo importan en puerto serie, así que se explica ahí.
            let es_serie = matches!(self.transporte(), pos_hw::Transporte::Serie { .. });
            ui.horizontal(|ui| {
                ui.label("Velocidad");
                ui.add_enabled_ui(es_serie, |ui| {
                    egui::ComboBox::from_id_salt("baudios_impresora")
                        .selected_text(format!("{} baudios", self.config.impresora_baudios))
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for b in pos_hw::BAUDIOS {
                                ui.selectable_value(
                                    &mut self.config.impresora_baudios,
                                    b as i64,
                                    format!("{b} baudios"),
                                );
                            }
                        });
                });
                ui.label(
                    egui::RichText::new(if es_serie {
                        "Si salen puros simbolos raros, prueba otra velocidad."
                    } else {
                        "Solo aplica a puerto serie."
                    })
                    .color(theme::texto_suave()),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Ancho del ticket");
                for (etiqueta, valor) in [("58 mm", 32i64), ("80 mm", 48i64)] {
                    if ui
                        .add(egui::SelectableLabel::new(
                            self.config.ancho_ticket == valor,
                            etiqueta,
                        ))
                        .clicked()
                    {
                        self.config.ancho_ticket = valor;
                    }
                }
                ui.checkbox(&mut self.config.abrir_cajon, "Abrir cajón con efectivo");
            });

            ui.horizontal(|ui| {
                if ui.add(theme::boton_acento("Imprimir prueba")).clicked() {
                    self.probar_impresion(false);
                }
                if ui.add(theme::boton_suave("Probar cajón")).clicked() {
                    self.probar_impresion(true);
                }
                ui.label(
                    egui::RichText::new(
                        "En el papel: acentos, regla, QR y corte. Vacío = vista previa.",
                    )
                    .color(theme::texto_suave()),
                );
            });
        });

        ui.add_space(10.0);
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Respaldo automático", Some(Icono::Escudo));
            ui.label(
                egui::RichText::new(
                    "Es la diferencia entre \"se murió la PC\" y \"perdimos el mes\". \
                     Se hace solo y nunca borra su última copia.",
                )
                .color(theme::texto_suave()),
            );
            ui.add_space(4.0);

            ui.checkbox(
                &mut self.config.respaldo_automatico,
                "Respaldar automaticamente (al arrancar y cada cierto tiempo)",
            );
            ui.checkbox(
                &mut self.config.respaldar_al_cortar,
                "Respaldar al cortar la caja (el mejor momento del dia)",
            );

            ui.horizontal(|ui| {
                ui.label("Guardar en");
                ui.add_sized(
                    [360.0, 26.0],
                    egui::TextEdit::singleline(&mut self.ruta_respaldo)
                        .hint_text("Carpeta (vacio = junto a la base de datos)"),
                );
            });

            // Unidades detectadas: si hay una memoria USB puesta, aparece aqui.
            if !self.unidades.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Unidades:").color(theme::texto_suave()));
                    for unidad in self.unidades.clone() {
                        let escogida = self.ruta_respaldo.trim() == unidad.ruta;
                        if ui
                            .add(egui::SelectableLabel::new(
                                escogida,
                                format!("{} ({})", unidad.etiqueta, unidad.ruta),
                            ))
                            .on_hover_text("Guardar los respaldos aqui")
                            .clicked()
                        {
                            self.ruta_respaldo = unidad.ruta.clone();
                            // Tambien se queda en la configuracion, no solo en pantalla.
                            self.config.respaldo_carpeta = unidad.ruta;
                        }
                    }
                });
            }

            ui.horizontal(|ui| {
                ui.label("Hacerlo cada");
                ui.add(
                    egui::DragValue::new(&mut self.config.respaldo_cada_dias)
                        .range(0..=30)
                        .suffix(" dias"),
                );
                ui.label("  y conservar los ultimos");
                ui.add(
                    egui::DragValue::new(&mut self.config.respaldos_a_conservar)
                        .range(1..=365)
                        .suffix(" respaldos"),
                );
            });

            ui.add_space(4.0);
            // Semáforo: cómo está el respaldo del negocio.
            let (color, texto) = if self.respaldos.total == 0 {
                (theme::peligro(), "Sin respaldos todavia".to_string())
            } else if self.respaldos.pendiente {
                (
                    theme::advertencia(),
                    format!(
                        "Pendiente: el ultimo es de hace mas de {} dia(s)",
                        self.respaldos.cada_dias.max(1)
                    ),
                )
            } else {
                (
                    theme::exito(),
                    match &self.respaldos.ultimo {
                        Some(u) => format!(
                            "Al dia. Ultimo: {} ({})",
                            FechaHora::cdmx(u.creado).fecha_hora(),
                            tamano(u.bytes)
                        ),
                        None => "Al dia".to_string(),
                    },
                )
            };
            ui.horizontal(|ui| {
                // El estado del respaldo en pastilla: es lo único que se revisa
                // de reojo una vez a la semana.
                theme::pastilla(ui, &texto, color);
                ui.label(
                    egui::RichText::new(format!(
                        "{} respaldos, {} en total",
                        self.respaldos.total,
                        tamano(self.respaldos.bytes)
                    ))
                    .color(theme::texto_suave()),
                );
            });

            ui.horizontal(|ui| {
                if ui.add(theme::boton_acento("Respaldar ahora")).clicked() {
                    self.respaldar_ahora();
                }
                if ui.add(theme::boton_suave("Verificar respaldos")).clicked() {
                    self.verificar_respaldos();
                }
            });

            if !self.respaldos.recientes.is_empty() {
                ui.collapsing(
                    format!("Ultimos respaldos ({})", self.respaldos.total),
                    |ui| {
                        for r in self.respaldos.recientes.clone() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(FechaHora::cdmx(r.creado).fecha_hora())
                                        .monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(tamano(r.bytes))
                                        .color(theme::texto_suave()),
                                );
                            });
                        }
                    },
                );
            }
        });

        ui.add_space(10.0);
        theme::marco_tarjeta(ui, |ui| {
            theme::seccion(ui, "Actualizaciones", Some(Icono::Foco));
            ui.label(
                egui::RichText::new(
                    "Sólo un paquete firmado (.posup). Guarda el que sí funciona, instala el nuevo \
                     y lo prueba: si no arranca, vuelve solo. La caja no se queda a medias.",
                )
                .color(theme::texto_suave()),
            );
            ui.add_space(4.0);

            ui.horizontal_wrapped(|ui| {
                tarjeta(ui, "Instalada", &self.actualizacion.version_instalada);
                tarjeta(
                    ui,
                    "Anterior guardada",
                    match self.actualizacion.version_respaldo.as_deref() {
                        Some(v) => v,
                        None => "—",
                    },
                );
            });
            if self.actualizacion.hay_pendiente {
                egui::Frame::new()
                    .fill(theme::tenir(theme::advertencia(), 0.14))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.colored_label(
                            theme::advertencia(),
                            "Quedó a medias (se apagó o no arrancó). Dale a «Deshacer la última actualización» \
                             y vuelve el que sí funcionaba.",
                        );
                    });
            }

            ui.horizontal(|ui| {
                ui.label("Paquete");
                ui.add_sized(
                    [420.0, 28.0],
                    egui::TextEdit::singleline(&mut self.ruta_paquete)
                        .hint_text("arrastra aqui el archivo .posup"),
                );
                if ui.button("Revisar").clicked() {
                    self.revisar_paquete();
                }
            });

            if let Some(p) = self.paquete.clone() {
                ui.add_space(4.0);
                if p.valido {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            theme::acento(),
                            format!("Firma valida · version {}", p.version),
                        );
                        ui.label(egui::RichText::new(format!("huella {}", p.huella)).monospace());
                    });
                    if !p.notas.is_empty() {
                        ui.label(egui::RichText::new(&p.notas).strong());
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Actualizar")
                                    .color(theme::tinta_sobre(theme::acento())),
                            )
                            .fill(theme::acento()),
                        )
                        .on_hover_text("Guarda la version actual, instala la nueva y la prueba")
                        .clicked()
                    {
                        self.aplicar_paquete();
                    }
                } else {
                    ui.colored_label(
                        theme::peligro(),
                        format!("No se puede instalar: {}", p.motivo),
                    );
                }
                ui.separator();
            }

            ui.horizontal_wrapped(|ui| {
                if self.actualizacion.hay_respaldo
                    && ui
                        .button("Deshacer la última actualización")
                        .on_hover_text("Vuelve al programa anterior (el que si funcionaba)")
                        .clicked()
                {
                    self.revertir_actualizacion();
                }
                ui.label(
                    egui::RichText::new(format!("Instalado en {}", self.actualizacion.ejecutable))
                        .weak(),
                );
            });
        });

        ui.add_space(10.0);
        if ui
            .add(theme::boton_acento("Guardar configuracion"))
            .clicked()
        {
            let config = self.config.clone();
            let r = self.api.ejecutar(Peticion {
                token: self.token(),
                comando: Comando::GuardarConfig { config },
            });
            self.mensaje = match r {
                pos_api::Respuesta::Ok(_) => "Configuración guardada".to_string(),
                pos_api::Respuesta::Error(e) => e,
            };
        }

        // Cómo se ve el programa. Va antes de la facturación: se toca una
        // vez, cuando se instala, y luego se olvida.
        self.seccion_apariencia(ui);

        // La facturación va al final: se usa una vez al día, no cada rato.
        self.seccion_facturacion(ui);
        self.seccion_acerca_de(ui);

        ui.separator();
        ui.collapsing("Atajos de teclado", |ui| {
            for (tecla, accion) in [
                ("F2", "Ir a venta / enfocar busqueda"),
                ("F3", "Catalogo"),
                ("F4", "Cobrar / confirmar el cobro"),
                ("Enter", "En cobro: confirmar. En venta: agregar"),
                ("+ / -", "Una pieza mas o menos del ultimo del ticket"),
                ("12*", "Las siguientes piezas son 12 (luego escanea)"),
                ("F5", "Conteo fisico"),
                ("F6", "Auditoria"),
                ("F7", "Clientes y cartera"),
                ("F8", "Reportes"),
                ("F9", "Caja (corte, retiros, arqueo)"),
                ("F10", "Configuracion"),
                ("Esc", "Cancelar / volver"),
                ("Supr", "Borrar ultimo producto del ticket"),
                ("Enter", "Agregar el codigo escaneado"),
                ("Cant.", "A la derecha de la busqueda: 12 y luego escanea"),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tecla).strong().color(theme::acento()));
                    ui.label(accion);
                });
            }
        });

        if let Some(previa) = self.vista_previa.clone() {
            ui.separator();
            ui.collapsing("Vista previa del ultimo ticket", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("previa")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.monospace(previa.as_str());
                    });
            });
        }
    }
}

impl eframe::App for PosApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.cargar_tras_entrar {
            self.cargar_tras_entrar = false;
            self.buscar("");
            self.refrescar_caja();
            self.refrescar_clientes();
            self.refrescar_categorias();
            self.refrescar_alertas();
            if !self.caja.abierta {
                self.mostrar_abrir_caja = true;
            }
        }
        let cobro_ya_abierto = self.mostrar_cobro;
        self.revisar_archivos_arrastrados(ctx);
        self.tick_teclado(ctx);
        self.barra_superior(ctx);

        if self.pantalla != Pantalla::Login {
            egui::TopBottomPanel::bottom("estado")
                .exact_height(28.0)
                .frame(
                    egui::Frame::new()
                        .fill(theme::fondo())
                        .inner_margin(egui::Margin::symmetric(16, 2)),
                )
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(ui.max_rect().left(), ui.max_rect().top()),
                            egui::vec2(ui.max_rect().width(), 1.0),
                        ),
                        egui::CornerRadius::ZERO,
                        theme::borde(),
                    );
                    ui.horizontal(|ui| {
                        if self.mensaje.is_empty() {
                            ui.label(
                                egui::RichText::new("● Listo para vender")
                                    .size(9.5)
                                    .strong()
                                    .color(theme::acento()),
                            );
                            ui.label(
                                egui::RichText::new("·")
                                    .size(9.5)
                                    .color(theme::texto_suave()),
                            );
                            ui.label(
                                egui::RichText::new(if self.caja.abierta {
                                    format!(
                                        "Caja abierta con fondo de {}",
                                        Money::from_cents(self.caja.fondo_cents).format_mxn()
                                    )
                                } else {
                                    "Caja cerrada · abre en F9".to_string()
                                })
                                .size(9.5)
                                .color(theme::texto_suave()),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(&self.mensaje)
                                    .size(9.5)
                                    .color(theme::acento()),
                            );
                        }
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("F2 Buscar   F4 Cobrar")
                                        .size(8.5)
                                        .color(theme::texto_suave()),
                                );
                            },
                        );
                    });
                });
        }

        // La pantalla de venta arma sus propios paneles (ticket a la derecha);
        // el resto vive dentro de un panel central.
        if self.pantalla == Pantalla::Venta {
            self.pantalla_venta(ctx);
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                self.cuerpo_de_la_pantalla(ui);
            });
        }

        if self.mostrar_cobro {
            self.ventana_cobro(ctx, cobro_ya_abierto);
        }
        if self.mostrar_abrir_caja {
            self.ventana_abrir_caja(ctx);
        }
        if self.mostrar_arqueo {
            self.ventana_arqueo(ctx);
        }
        if self.mostrar_firma_conteo {
            self.ventana_firma_conteo(ctx);
        }
        if self.mostrar_mapeo {
            self.ventana_mapeo(ctx);
        }
        if self.ultimo_cierre.is_some() {
            self.ventana_cierre(ctx);
        }

        // egui sólo repinta cuando hay interacción; el reloj de la barra se
        // refresca cada medio minuto. En un Celeron esto es la diferencia entre
        // 0 % y 15 % de CPU en reposo.
        ctx.request_repaint_after(Duration::from_secs(30));
    }
}

/// Nombre legible del método de pago tal como viaja en el DTO.
fn etiqueta_metodo(metodo: &str) -> &'static str {
    match metodo {
        "efectivo" => "EFECTIVO",
        "tarjeta" => "TARJETA",
        "transferencia" => "TRANSFERENCIA",
        "vales" => "VALES",
        "credito" => "CREDITO",
        _ => "OTRO",
    }
}

// ------------------------------------------------------------------ utilidades

fn fila(ui: &mut egui::Ui, etiqueta: &str, valor: &str, color: Option<egui::Color32>) {
    ui.horizontal(|ui| {
        ui.label(etiqueta);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match color {
                Some(c) => {
                    ui.colored_label(c, valor);
                }
                None => {
                    ui.label(valor);
                }
            };
        });
    });
}

fn fila_monto(ui: &mut egui::Ui, etiqueta: &str, centavos: i64, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(etiqueta);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.colored_label(color, Money::from_cents(centavos).format_mxn());
        });
    });
}

/// "12*" o "*12" en la búsqueda: las siguientes piezas, no un producto.
fn cantidad_desde_atajo(texto: &str) -> Option<String> {
    let t = texto.trim();
    let n = t
        .strip_suffix('*')
        .or_else(|| t.strip_prefix('*'))
        .unwrap_or("");
    if !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit())
        && (t.starts_with('*') || t.ends_with('*'))
    {
        Some(n.to_string())
    } else {
        None
    }
}

/// Milésimas a texto del cajero: "12", "3.5", "-4".
fn cantidad(millis: i64) -> String {
    let entero = millis / 1_000;
    let fraccion = (millis % 1_000).abs();
    if fraccion == 0 {
        format!("{entero}")
    } else {
        // Un decimal basta: nadie cuenta 3.1416 piezas.
        format!("{entero}.{}", fraccion / 100)
    }
}

fn tarjeta(ui: &mut egui::Ui, titulo: &str, valor: &str) {
    theme::marco_tarjeta(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(titulo)
                    .size(12.0)
                    .color(theme::texto_suave()),
            );
            ui.label(
                egui::RichText::new(valor)
                    .size(20.0)
                    .strong()
                    .color(theme::texto()),
            );
        });
    });
}

#[cfg(test)]
thread_local! {
    /// Hora fija para las capturas. Si está puesta, `ahora_cdmx` la devuelve
    /// en vez de la hora real: si no, cada imagen cambiaría minuto a minuto y
    /// las pruebas de captura estarían siempre fallando por nada.
    #[allow(clippy::missing_const_for_thread_local)]
    static HORA_FIJA: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Fija la hora que ven las pruebas (`None` para volver a la real).
#[cfg(test)]
pub fn fijar_hora(hora: Option<&str>) {
    HORA_FIJA.with(|h| *h.borrow_mut() = hora.map(|s| s.to_string()));
}

/// Fecha y hora local de la Ciudad de México sin dependencias externas
/// (chrono pesa más que esta función y aquí cada kilobyte cuenta).
pub fn ahora_cdmx() -> String {
    #[cfg(test)]
    if let Some(fija) = HORA_FIJA.with(|h| h.borrow().clone()) {
        return fija;
    }
    let segs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        - 6 * 3600; // UTC-6, horario de invierno de la Ciudad de México
    let dias = segs.div_euclid(86_400);
    let resto = segs.rem_euclid(86_400);
    let (anio, mes, dia) = civil_desde_dias(dias);
    format!(
        "{anio:04}-{mes:02}-{dia:02} {:02}:{:02}",
        resto / 3600,
        (resto % 3600) / 60
    )
}

/// Algoritmo civil_from_days de Howard Hinnant (dominio público).
fn civil_desde_dias(dias: i64) -> (i64, u32, u32) {
    let z = dias + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doce_asterisco_es_la_cantidad_no_un_producto() {
        assert_eq!(cantidad_desde_atajo("12*").as_deref(), Some("12"));
        assert_eq!(cantidad_desde_atajo("*6").as_deref(), Some("6"));
        assert_eq!(cantidad_desde_atajo("7501"), None);
        assert_eq!(cantidad_desde_atajo("12"), None);
    }

    #[test]
    fn las_cantidades_se_leen_como_las_dice_el_cajero() {
        assert_eq!(cantidad(0), "0");
        assert_eq!(cantidad(1_000), "1");
        assert_eq!(cantidad(12_000), "12");
        assert_eq!(cantidad(1_500), "1.5");
        assert_eq!(cantidad(3_750), "3.7");
        // Lo que falta se ve en negativo: es una merma.
        assert_eq!(cantidad(-4_000), "-4");
        assert_eq!(cantidad(-4_500), "-4.5");
    }

    #[test]
    fn la_fecha_sale_en_formato_legible() {
        let f = ahora_cdmx();
        assert_eq!(f.len(), 16);
        assert!(f.contains('-') && f.contains(':'));
    }

    #[test]
    fn conversion_civil_correcta() {
        assert_eq!(civil_desde_dias(20_736), (2026, 10, 10));
        assert_eq!(civil_desde_dias(0), (1970, 1, 1));
        assert_eq!(civil_desde_dias(-719_162), (1, 1, 1));
        assert_eq!(civil_desde_dias(19_782), (2024, 2, 29)); // año bisiesto
    }

    struct ApiMuda;
    impl PosApi for ApiMuda {
        fn ejecutar(&mut self, _: Peticion) -> pos_api::Respuesta {
            pos_api::Respuesta::error("muda")
        }
    }

    fn app_muda() -> PosApp {
        PosApp::new(Box::new(ApiMuda), false)
    }

    fn coca(stock: i64, track: bool) -> ProductoDto {
        ProductoDto {
            id: 1,
            sku: "7501".into(),
            name: "Coca".into(),
            price_cents: 1_800,
            cost_cents: 900,
            tax_bp: 1_600,
            stock_millis: stock,
            min_stock_millis: 0,
            track_inventory: track,
            clave_sat: "01010101".into(),
            clave_unidad: "H87".into(),
            unidad_sat: "Pieza".into(),
        }
    }

    #[test]
    fn se_agrega_la_cantidad_escrita_de_un_golpe() {
        let mut app = app_muda();
        app.venta_cant = "12".into();
        app.agregar_producto(&coca(100_000, true));
        assert_eq!(app.venta.lines.len(), 1);
        assert_eq!(app.venta.lines[0].qty.millis(), 12_000);
        assert_eq!(app.venta_cant, "1", "después vuelve a una pieza");
    }

    #[test]
    fn no_se_meten_doce_si_solo_hay_tres() {
        let mut app = app_muda();
        app.venta_cant = "12".into();
        app.agregar_producto(&coca(3_000, true));
        assert!(app.venta.is_empty());
        assert!(app.mensaje.contains("3"), "{}", app.mensaje);
    }

    #[test]
    fn no_se_agrega_mas_de_lo_que_hay_en_el_anaquel() {
        let mut app = app_muda();
        let p = coca(1_000, true);
        app.agregar_producto(&p);
        assert_eq!(app.venta.lines.len(), 1);
        assert_eq!(app.venta.lines[0].qty.millis(), 1_000);
        app.agregar_producto(&p);
        assert_eq!(app.venta.lines[0].qty.millis(), 1_000);
        assert!(
            app.mensaje.contains("Coca") && app.mensaje.contains("1"),
            "el cajero ve qué falta: {}",
            app.mensaje
        );
    }

    #[test]
    fn con_el_anaquel_vacio_no_entra_al_ticket() {
        let mut app = app_muda();
        app.agregar_producto(&coca(0, true));
        assert!(app.venta.is_empty());
        assert!(app.mensaje.contains("Coca"));
    }

    #[test]
    fn un_servicio_sin_inventario_si_entra_varias_veces() {
        let mut app = app_muda();
        let p = coca(0, false);
        for _ in 0..5 {
            app.agregar_producto(&p);
        }
        assert_eq!(app.venta.lines.len(), 1);
        assert_eq!(app.venta.lines[0].qty.millis(), 5_000);
        assert!(app.mensaje.is_empty());
    }

    #[test]
    fn el_precio_a_mano_tiene_etiqueta_en_espanol() {
        assert_eq!(etiqueta_accion("precio"), "Precio a mano");
        assert_eq!(etiqueta_accion("descuento"), "Descuento");
        assert_eq!(etiqueta_accion("fiado"), "Fiado");
        assert_eq!(etiqueta_accion("pac_http"), "PAC por HTTP bloqueado");
        assert_eq!(etiqueta_accion("impresion_prueba"), "Ticket de prueba");
        assert_eq!(
            etiqueta_accion("impresion_prueba_fallida"),
            "Ticket de prueba fallido"
        );
        assert_eq!(etiqueta_accion("autorizar"), "Autorización");
    }

    #[test]
    fn el_alta_rapida_pide_nombre_antes_que_nada() {
        let mut app = app_muda();
        app.guardar_alta_rapida();
        assert!(!app.alta_ok);
        assert!(
            app.alta_mensaje.contains("vend"),
            "habla de tienda, no de campos: {}",
            app.alta_mensaje
        );
    }

    #[test]
    fn el_alta_rapida_pide_precio_valido() {
        let mut app = app_muda();
        app.alta_nombre = "Coca 600 ml".into();
        app.guardar_alta_rapida();
        assert!(!app.alta_ok);
        assert!(app.alta_mensaje.contains("cuánto"), "{}", app.alta_mensaje);

        app.alta_precio = "0".into();
        app.guardar_alta_rapida();
        assert!(!app.alta_ok);
        assert!(app.alta_mensaje.contains("$0.00"), "{}", app.alta_mensaje);
    }

    #[test]
    fn el_alta_rapida_rechaza_stock_ilegible() {
        let mut app = app_muda();
        app.alta_nombre = "Pan".into();
        app.alta_precio = "10".into();
        app.alta_stock = "-5".into();
        app.guardar_alta_rapida();
        assert!(!app.alta_ok);
        assert!(app.alta_mensaje.contains("Cuántos"), "{}", app.alta_mensaje);
    }
}
