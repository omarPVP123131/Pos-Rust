//! Objetos de transferencia: lo que viaja entre la interfaz y los servicios.
//!
//! Regla: **sólo tipos simples** (i64, String, bool, Vec). Nada de `Money`,
//! nada de `Connection`. Así el mismo comando se serializa a JSON para la app
//! móvil o se llama en proceso sin costo.
//!
//! Todo el dinero va en centavos y las cantidades en milésimos, igual que en
//! el resto del sistema.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct UsuarioDto {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SesionDto {
    pub usuario: UsuarioDto,
    /// Token opaco. El cliente lo manda en cada petición siguiente.
    pub token: String,
}

/// Credenciales de un supervisor para autorizar una operación sensible.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CredencialesDto {
    pub username: String,
    pub pin: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProductoDto {
    pub id: i64,
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
    pub cost_cents: i64,
    pub tax_bp: i32,
    pub stock_millis: i64,
    pub min_stock_millis: i64,
    pub track_inventory: bool,
    /// Clave de `c_ClaveProdServ`. '01010101' = no existe en el catálogo.
    pub clave_sat: String,
    /// Clave de `c_ClaveUnidad`. 'H87' = pieza.
    pub clave_unidad: String,
    /// Cómo se nombra la unidad en el comprobante: "Pieza", "Kilo"...
    pub unidad_sat: String,
}

/// Cuánto se devuelve de un producto en una nota de crédito parcial.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LineaDevolucionDto {
    pub sku: String,
    /// Cantidad en milésimos: 2 piezas = 2000, 1.5 kg = 1500.
    pub cantidad_millis: i64,
}

/// Un ticket ya guardado, listo para reimprimirse o devolverse.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TicketDto {
    pub folio: String,
    pub fecha: String,
    pub cajero: String,
    pub cliente: String,
    pub estado: String,
    pub lineas: Vec<TicketLineaDto>,
    pub subtotal_cents: i64,
    pub descuento_cents: i64,
    pub impuesto_cents: i64,
    pub total_cents: i64,
    pub pagos: Vec<PagoDto>,
    pub cambio_cents: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TicketLineaDto {
    pub sku: String,
    pub descripcion: String,
    pub qty_millis: i64,
    pub precio_cents: i64,
    pub descuento_cents: i64,
    pub importe_cents: i64,
}

/// Cuánto vendió cada cajero en el día.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CajeroDto {
    pub nombre: String,
    pub tickets: i64,
    pub vendido_cents: i64,
    pub devuelto_cents: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CategoriaDto {
    pub id: i64,
    pub name: String,
    pub productos: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct NuevoProductoDto {
    /// `Some(id)` edita el producto existente; `None` lo da de alta.
    pub id: Option<i64>,
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
    pub cost_cents: i64,
    pub tax_bp: i32,
    pub unit: String,
    pub stock_millis: i64,
    pub min_stock_millis: i64,
    pub track_inventory: bool,
    pub barcodes: Vec<String>,
    /// Nombre de la categoría. Si no existe, se crea.
    pub categoria: Option<String>,
    /// Claves del SAT. `None` = déjalas como están (o usa las genéricas).
    pub clave_sat: Option<String>,
    pub clave_unidad: Option<String>,
    pub unidad_sat: Option<String>,
}

/// Una línea del carrito. `descripcion` y `impuesto_bp` van por si el cliente
/// móvil no tiene catálogo local, pero el servidor **siempre** revalida contra
/// la base de datos.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LineaDto {
    pub sku: String,
    pub descripcion: String,
    pub qty_millis: i64,
    pub precio_cents: i64,
    pub descuento_cents: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PagoDto {
    /// efectivo | tarjeta | transferencia | vales | credito
    pub metodo: String,
    pub monto_cents: i64,
    /// Cuánto entregó el cliente (sólo efectivo).
    pub recibido_cents: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VentaDto {
    pub lineas: Vec<LineaDto>,
    pub descuento_global_cents: i64,
    pub pagos: Vec<PagoDto>,
    pub cliente_id: Option<i64>,
    pub nota: String,
    /// Ya no se pide. Se ignora: queda anotado el cajero que cobró.
    pub supervisor: Option<CredencialesDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VentaGuardadaDto {
    pub id: i64,
    pub folio: String,
    pub subtotal_cents: i64,
    pub impuesto_cents: i64,
    pub descuento_cents: i64,
    pub total_cents: i64,
    pub cambio_cents: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct EstadoCajaDto {
    pub abierta: bool,
    pub sesion_id: Option<i64>,
    pub fondo_cents: i64,
    pub ventas_efectivo_cents: i64,
    pub entradas_cents: i64,
    pub retiros_cents: i64,
    pub gastos_cents: i64,
    pub esperado_cents: i64,
    pub tickets: i64,
    pub vendido_cents: i64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TotalesDto {
    pub count: i64,
    pub subtotal_cents: i64,
    pub tax_cents: i64,
    pub discount_cents: i64,
    pub total_cents: i64,
    pub efectivo_cents: i64,
    pub tarjeta_cents: i64,
    pub transferencia_cents: i64,
    pub otros_cents: i64,
    pub costo_cents: i64,
    pub utilidad_cents: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VentaResumenDto {
    pub id: i64,
    pub folio: String,
    pub created_at: String,
    pub total_cents: i64,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopProductoDto {
    pub nombre: String,
    pub cantidad_millis: i64,
    pub importe_cents: i64,
}

/// Margen de un producto en el día (costo histórico de la venta).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct UtilidadProductoDto {
    pub sku: String,
    pub nombre: String,
    pub cantidad_millis: i64,
    pub vendido_cents: i64,
    pub costo_cents: i64,
    pub utilidad_cents: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClienteDto {
    pub id: i64,
    pub name: String,
    pub phone: String,
    pub rfc: String,
    pub limite_cents: i64,
    pub saldo_cents: i64,
    pub disponible_cents: i64,
    /// Clave de `c_RegimenFiscal`. Vacío = no se le puede facturar todavía.
    pub regimen_fiscal: String,
    /// Código postal de su domicilio fiscal.
    pub codigo_postal: String,
    /// Clave de `c_UsoCFDI` (S01, G03, G01...).
    pub uso_cfdi: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct NuevoClienteDto {
    pub name: String,
    pub rfc: String,
    pub phone: String,
    pub email: String,
    pub credit_limit_cents: i64,
    pub regimen_fiscal: String,
    pub codigo_postal: String,
    pub uso_cfdi: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfigDto {
    pub tienda: String,
    pub direccion: String,
    pub pie: String,
    pub impresora: String,
    pub ancho_ticket: i64,
    // --- impresora: cómo hablarle (no todas las térmicas son iguales) ---
    /// Perfil: "Genérica 80 mm", "Epson TM (T20/T82/T88)"...
    pub impresora_perfil: String,
    /// Baudios del puerto serie (9600 en las chinas, 115200 en Epson).
    pub impresora_baudios: i64,
    /// Abrir el cajón automáticamente cuando hay pago en efectivo.
    pub abrir_cajon: bool,
    // --- respaldo automático ---
    /// Hacer el respaldo solo, sin que nadie se acuerde.
    pub respaldo_automatico: bool,
    /// Carpeta donde se guardan (puede ser una memoria USB).
    pub respaldo_carpeta: String,
    /// Cada cuántos días. 0 = cada vez que arranca el programa.
    pub respaldo_cada_dias: i64,
    /// Cuántos respaldos se conservan antes de borrar el más viejo.
    pub respaldos_a_conservar: i64,
    /// Respaldar al cerrar la caja (el mejor momento del día).
    pub respaldar_al_cortar: bool,
    /// Puerto del servidor LAN para el celular. 0 = 8080.
    pub puerto_lan: i64,
}

/// Una impresora que el programa encontró conectada al equipo.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ImpresoraDto {
    /// "/dev/usb/lp0", "/dev/ttyUSB0", "192.168.1.50:9100", "POS-80"...
    pub ruta: String,
    /// "Epson (USB /dev/usb/lp0)".
    pub etiqueta: String,
    pub fabricante: String,
    /// "usb", "serie", "red" o "windows".
    pub transporte: String,
    pub baudios: i64,
    /// Perfil sugerido para esta impresora.
    pub perfil: String,
    /// ¿Es una impresora de verdad (clase USB 07) o sólo un puerto?
    pub es_impresora: bool,
    /// Qué se supo y qué no, para mostrarlo en la pantalla.
    pub nota: String,
}

/// Resultado de una impresión de prueba.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PruebaImpresionDto {
    pub ok: bool,
    /// "Ticket de prueba enviado a Serie /dev/ttyUSB0 a 9600 baudios (412 bytes)."
    pub mensaje: String,
    pub bytes: i64,
    /// Descripción del transporte que se usó.
    pub transporte: String,
    pub perfil: String,
}

/// Un respaldo ya hecho, para mostrarse en la interfaz.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RespaldoDto {
    pub nombre: String,
    pub ruta: String,
    pub bytes: u64,
    /// Segundos desde el epoch: la interfaz lo formatea.
    pub creado: i64,
}

/// El estado de los respaldos: lo que ve el cajero en Configuración.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct EstadoRespaldosDto {
    pub carpeta: String,
    pub total: usize,
    pub bytes: u64,
    pub ultimo: Option<RespaldoDto>,
    /// `true` si ya toca respaldar (no hay ninguno o está muy viejo).
    pub pendiente: bool,
    pub automatico: bool,
    pub cada_dias: i64,
    pub conservar: i64,
    /// Qué tan sano está el respaldo más reciente: `None` si nunca se revisó.
    pub ultimo_verificado: Option<bool>,
    /// Último aviso o resultado ("Respaldo listo: respaldo-....db").
    pub mensaje: String,
    /// Los respaldos más recientes, para listarlos en la pantalla.
    pub recientes: Vec<RespaldoDto>,
}

/// Un lugar donde guardar el respaldo: memoria USB, disco o carpeta.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct UnidadDto {
    pub etiqueta: String,
    pub ruta: String,
}

/// Un renglón de la bitácora, listo para mostrarse.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EventoAuditoriaDto {
    pub id: i64,
    pub usuario: String,
    /// Clave de la acción (`venta`, `autorizar`...).
    pub accion: String,
    /// Etiqueta en español ya traducida.
    pub etiqueta: String,
    pub detalle: String,
    /// Hora de la tienda: `YYYY-MM-DD HH:MM:SS`.
    pub cuando: String,
}

/// La respuesta de la auditoría: eventos, acciones disponibles y total.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct EventosAuditoriaDto {
    pub eventos: Vec<EventoAuditoriaDto>,
    /// Acciones que de verdad hay en la bitácora (para el filtro).
    pub acciones: Vec<String>,
    /// Cuántos hay en total, sin filtro.
    pub total: i64,
    /// Cuántos trajo este filtro.
    pub mostrados: usize,
}

/// Cómo se leyó un CSV, para elegir el mapeo a mano antes de importar.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AnalisisCsvDto {
    pub encabezados: Vec<String>,
    /// Campo sugerido por columna, en el mismo orden.
    pub sugerido: Vec<String>,
    /// Primeros renglones de ejemplo.
    pub ejemplo: Vec<Vec<String>>,
    pub separador: char,
    pub renglones: usize,
    pub tiene_nombre: bool,
    /// Los campos que se pueden elegir: (clave, etiqueta).
    pub campos: Vec<(String, String)>,
}

/// Un producto dentro del conteo físico.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RenglonConteoDto {
    pub product_id: i64,
    pub sku: String,
    pub nombre: String,
    pub unidad: String,
    pub categoria: Option<String>,
    /// Lo que el sistema decía al empezar (milésimas).
    pub esperado_millis: i64,
    /// Lo que contó la persona; `None` si aún no se cuenta.
    pub contado_millis: Option<i64>,
    /// El stock en este momento.
    pub actual_millis: i64,
    /// `contado - actual`; `None` si no se ha contado.
    pub diferencia_millis: Option<i64>,
}

/// Un conteo físico completo: cómo va y qué se ha contado.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ConteoDto {
    pub id: i64,
    pub estado: String,
    pub alcance: String,
    pub filtro: String,
    pub created_at: String,
    pub renglones: Vec<RenglonConteoDto>,
    pub productos: usize,
    pub contados: usize,
    pub con_diferencia: usize,
    pub faltantes_millis: i64,
    pub sobrantes_millis: i64,
    pub valor_merma_cents: i64,
}

/// Lo que se supo de un paquete de actualización, para que el encargado decida.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PaqueteDto {
    pub ruta: String,
    /// `true` sólo si la firma, el hash y la versión están bien.
    pub valido: bool,
    pub version: String,
    pub notas: String,
    pub tamanio: u64,
    /// Huella corta del ejecutable, para confirmar que es el que le dieron.
    pub huella: String,
    /// Por qué no se puede instalar (vacío cuando sí se puede).
    pub motivo: String,
}

/// Cómo está instalado el programa.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct EstadoActualizacionDto {
    pub version_instalada: String,
    pub ejecutable: String,
    /// Hay una versión anterior guardada: se puede deshacer.
    pub hay_respaldo: bool,
    /// La última actualización no se confirmó: algo quedó a medias.
    pub hay_pendiente: bool,
    pub version_respaldo: Option<String>,
}

/// Los datos fiscales de la tienda: con esto se arma el comprobante.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ConfigFiscalDto {
    pub rfc: String,
    pub nombre: String,
    /// Clave de tres dígitos del catálogo c_RegimenFiscal.
    pub regimen_fiscal: String,
    pub codigo_postal: String,
    /// c_FormaPago: 01 efectivo, 04 tarjeta de crédito, 28 débito, 99 otros.
    pub forma_pago: String,
    /// c_Periodicidad de la factura global: 01 diario, 04 mensual...
    pub periodicidad: String,
    /// Donde se dejan los XML por timbrar.
    pub carpeta_salida: String,
    /// Donde aparecen los XML ya timbrados.
    pub carpeta_entrada: String,
}

/// Una factura en la cola, timbrada o esperando timbre.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FacturaDto {
    pub id: i64,
    pub folio: String,
    pub tipo: String,
    pub periodo: String,
    pub periodicidad: String,
    pub receptor_rfc: String,
    pub subtotal_cents: i64,
    pub iva_cents: i64,
    pub total_cents: i64,
    /// Folios de las ventas que metió, separados por coma.
    pub ventas: String,
    pub estado: String,
    pub uuid: Option<String>,
    pub ultimo_error: Option<String>,
    pub intentos: i64,
    pub created_at: String,
    pub timbrada_at: Option<String>,
}

/// Un proveedor: de quién se pide la mercancía.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ProveedorDto {
    pub id: i64,
    pub name: String,
    pub phone: String,
    pub nota: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct NuevoProveedorDto {
    /// `None` da de alta; `Some(id)` edita.
    pub id: Option<i64>,
    pub name: String,
    pub phone: String,
    pub nota: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LineaPedidoDto {
    pub product_id: i64,
    pub sku: String,
    pub nombre: String,
    pub qty_pedida_millis: i64,
    pub qty_recibida_millis: i64,
    pub costo_cents: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PedidoDto {
    pub id: i64,
    pub folio: String,
    pub proveedor_id: i64,
    pub proveedor: String,
    pub estado: String,
    pub nota: String,
    pub created_at: String,
    pub total_costo_cents: i64,
    pub lineas: Vec<LineaPedidoDto>,
}

/// Lo que llegó en esta entrega.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct EntregaDto {
    pub product_id: i64,
    pub qty_millis: i64,
}
