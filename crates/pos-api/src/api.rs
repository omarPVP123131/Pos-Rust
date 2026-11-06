//! El contrato único: comandos, respuestas y el trait `PosApi`.

use serde::{Deserialize, Serialize};

use crate::dto::*;
use crate::importar::ResultadoImporte;

/// Petición: un comando + el token de la sesión (si ya inició).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peticion {
    pub token: Option<String>,
    pub comando: Comando,
}

/// Todo lo que se puede pedirle al punto de venta.
///
/// Es la lista completa de la operación: si una interfaz nueva implementa
/// estos comandos, es un punto de venta funcional.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Comando {
    // --- sesión ---
    /// Lo que la pantalla de login necesita: el nombre de usuario y cómo se
    /// llama cada quién.
    ///
    /// Ésta es la lista **pública**, la única que se puede pedir sin haber
    /// iniciado sesión, y por eso no trae roles ni quién está dado de alta. La
    /// lista completa es `Comando::Usuarios`, y ésa sí pide permiso.
    UsuariosLogin,
    /// Los usuarios con todo: roles y altas/bajas. Pide `Permiso::Usuarios`,
    /// porque con esto se sabe quién manda en la tienda.
    Usuarios,
    CrearUsuario {
        username: String,
        display_name: String,
        pin: String,
        role: String,
    },
    ActualizarUsuario {
        id: i64,
        display_name: String,
        role: String,
    },
    CambiarPinUsuario {
        id: i64,
        pin: String,
    },
    DesactivarUsuario {
        id: i64,
    },
    DesbloquearUsuario {
        id: i64,
    },
    Login {
        username: String,
        pin: String,
    },
    Salir,

    // --- catálogo ---
    BuscarProductos {
        termino: String,
        limite: usize,
    },
    ProductoPorCodigo {
        codigo: String,
    },
    GuardarProducto {
        producto: NuevoProductoDto,
    },
    /// Categorías con su conteo de productos.
    Categorias,
    GuardarCategoria {
        nombre: String,
    },
    /// Productos en o por debajo del mínimo: la alerta del mostrador.
    ProductosBajoMinimo,
    /// Importación masiva desde un CSV. **Todo o nada**: si un renglón está
    /// mal, no se escribe nada y el resultado trae la lista de errores.
    /// Lee el encabezado y un ejemplo, para elegir el mapeo antes de importar.
    AnalizarCsv {
        texto: String,
    },
    // --- actualizaciones ---
    /// Lee un paquete `.posup` y dice si se puede instalar, sin tocar nada.
    RevisarActualizacion {
        ruta: String,
    },
    /// Instala un paquete firmado: respalda, cambia, prueba y, si algo falla,
    /// se deshace.
    AplicarActualizacion {
        ruta: String,
    },
    /// Vuelve al ejecutable anterior. El botón del pánico.
    RevertirActualizacion,
    /// Versión instalada, si hay respaldo y si quedó algo pendiente.
    EstadoActualizacion,
    ImportarCsv {
        texto: String,
        /// Sobreescribir los productos cuyo SKU ya existe.
        actualizar: bool,
        /// Un campo por columna: `"nombre"`, `"precio"`, `"ignorar"`...
        /// Vacío = adivinar como siempre.
        mapeo: Vec<String>,
    },
    // --- conteo físico ---
    /// Empieza un conteo: toma la foto de lo que el sistema cree que hay.
    IniciarConteo {
        categoria: Option<String>,
    },
    /// El conteo abierto (o uno por id), con sus renglones y su resumen.
    VerConteo {
        conteo_id: Option<i64>,
    },
    /// Anota lo contado. Busca por SKU **o** código de barras, que es como se
    /// cuenta de verdad: escaneando.
    ContarProducto {
        codigo: String,
        cantidad_millis: i64,
    },
    /// Cierra el conteo. Con `aplicar`, escribe los ajustes de inventario.
    CerrarConteo {
        conteo_id: i64,
        aplicar: bool,
        supervisor: Option<CredencialesDto>,
    },
    /// Cancela un conteo abierto: tira lo contado, no toca el inventario.
    CancelarConteo {
        conteo_id: i64,
    },
    /// El catálogo completo como CSV, para respaldo o para editarlo en Excel.
    ExportarCsv,
    /// Dar de baja un producto (nunca se borra: hay ventas que lo referencian).
    DesactivarProducto {
        id: i64,
    },
    /// Entrada de mercancía, ajuste o merma. El motivo es obligatorio.
    AjusteInventario {
        producto_id: i64,
        /// Positivo entra mercancía, negativo sale. En milésimas.
        delta_millis: i64,
        motivo: String,
    },

    // --- facturación electrónica (CFDI 4.0) ---
    /// Los datos fiscales de la tienda.
    ConfigFiscal,
    GuardarConfigFiscal {
        config: ConfigFiscalDto,
    },
    /// Junta las ventas de un día en una factura global y la deja en la cola.
    FacturaGlobal {
        /// `AAAA-MM-DD`, día civil de la tienda (no UTC).
        fecha: String,
        /// c_Periodicidad: 01 diario, 02 semanal, 04 mensual, 05 bimestral.
        periodicidad: String,
    },
    /// Las facturas en la cola, las más nuevas primero.
    Facturas {
        limite: usize,
    },
    /// Reintenta el timbrado. Si el PAC no responde, la deja pendiente.
    ReintentarFactura {
        id: i64,
    },
    /// El XML de una factura: el timbrado si ya se timbró, si no, el generado.
    XmlFactura {
        id: i64,
    },
    /// Factura individual: una venta a nombre de un cliente con RFC.
    ///
    /// Esa venta deja de entrar en la factura global del día, porque si
    /// entrara el SAT vería el mismo ingreso dos veces.
    FacturarVenta {
        folio: String,
        /// Día de la factura, `AAAA-MM-DD`.
        fecha: String,
    },
    /// Cuántas ventas con RFC siguen esperando su factura.
    ///
    /// Esas ventas NO van en la global, así que si nadie las factura ese
    /// ingreso se queda sin reportar. La pantalla lo tiene que decir.
    VentasPorFacturar,
    /// Complemento de pagos: el cliente pagó algo que le facturamos a crédito.
    ///
    /// Cuando se facturó PPD (pago en parcialidades o diferido), la factura
    /// queda a deber. Cada vez que el cliente entrega dinero se emite uno de
    /// estos, ligado por UUID a las facturas que va cubriendo.
    ComplementoPago {
        cliente_id: i64,
        /// Cuánto pagó, en centavos.
        monto_cents: i64,
        /// Día del pago (YYYY-MM-DD).
        fecha: String,
        /// Clave SAT de la forma en que pagó: 01 efectivo, 03 transferencia.
        forma_pago: String,
    },
    /// Cuánto siguen debiendo las facturas a crédito de este cliente, en centavos.
    SaldoPorPagar {
        cliente_id: i64,
    },

    /// Nota de crédito por **monto**, no por piezas: "te devuelvo $50".
    ///
    /// Para ropa y a granel, donde contar piezas no describe la devolución.
    NotaCreditoMonto {
        folio: String,
        /// Día de la nota, `AAAA-MM-DD`.
        fecha: String,
        /// Cuánto dinero se devuelve, en centavos.
        monto_cents: i64,
    },
    /// Nota de crédito de una venta ya facturada (devolución con comprobante).
    ///
    /// Va ligada por UUID a la factura que la originó. Si la venta no se ha
    /// facturado, no hay nota: se explica en vez de fabricar una inválida.
    NotaCredito {
        folio: String,
        /// Día de la nota, `AAAA-MM-DD`.
        fecha: String,
        /// Qué se devuelve. Vacío = todo lo que quede sin devolver.
        lineas: Vec<LineaDevolucionDto>,
    },

    // --- venta ---
    Cobrar {
        venta: VentaDto,
    },
    Devolver {
        folio: String,
        supervisor: Option<CredencialesDto>,
    },
    VentasRecientes {
        limite: usize,
    },
    /// Un ticket ya guardado, para reimprimirlo.
    TicketPorFolio {
        folio: String,
    },
    /// Ventas del día por cajero.
    TotalesPorCajero,
    /// Ventas de un rango como CSV (para el contador).
    ExportarVentasCsv {
        /// `YYYY-MM-DD`. Vacío = hoy.
        desde: String,
        hasta: String,
    },

    // --- caja ---
    EstadoCaja,
    AbrirCaja {
        fondo_cents: i64,
    },
    CerrarCaja {
        /// (denominación en centavos, cantidad de piezas)
        piezas: Vec<(i64, i64)>,
        supervisor: Option<CredencialesDto>,
    },
    MovimientoCaja {
        /// entrada | retiro | gasto
        tipo: String,
        monto_cents: i64,
        motivo: String,
        supervisor: Option<CredencialesDto>,
    },

    // --- clientes y cartera ---
    Clientes {
        termino: String,
    },
    GuardarCliente {
        cliente: NuevoClienteDto,
    },
    Abonar {
        cliente_id: i64,
        monto_cents: i64,
        metodo: String,
    },

    /// Copia consistente de la base (`VACUUM INTO`), verificada al terminar.
    Respaldo {
        ruta: String,
    },
    /// Estado de los respaldos: cuántos hay, cuál es el último y si ya toca.
    Respaldos,
    /// Hace un respaldo ya y rota los viejos. Si no se manda carpeta, usa la
    /// de la configuración.
    RespaldarAhora {
        carpeta: Option<String>,
    },
    /// Abre todos los respaldos y comprueba que sirvan.
    VerificarRespaldos,
    /// Unidades disponibles para guardar (memorias USB, discos...).
    Unidades,
    /// Bitácora: quién hizo qué y quién autorizó qué.
    Auditoria {
        desde: String,
        hasta: String,
        /// Nombre o usuario; vacío = todos.
        usuario: String,
        /// Acción exacta (`venta`, `autorizar`...); vacío = todas.
        accion: String,
        /// Texto libre dentro del detalle.
        texto: String,
        limite: usize,
    },

    // --- reportes y configuración ---
    TotalesHoy,
    TopProductos {
        limite: usize,
    },
    /// Utilidad por producto del día (costo histórico, no el de ahora).
    UtilidadHoy {
        limite: usize,
    },
    /// Siguiente EAN-13 interno (prefijo 20) que no esté usado.
    CodigoPropio,
    /// Imprime una etiqueta EAN-13 en la térmica de la tienda.
    ImprimirEtiqueta {
        codigo: String,
        nombre: String,
        precio_cents: i64,
    },
    /// Códigos de barras de un producto (el primero, para editar).
    CodigosDe {
        producto_id: i64,
    },
    /// Cancela un CFDI: motivo SAT y, si es 01, UUID de la que sustituye.
    CancelarFactura {
        id: i64,
        motivo: String,
        uuid_sustitucion: String,
    },
    LeerConfig,
    GuardarConfig {
        config: ConfigDto,
    },

    // --- pedidos a proveedor ---
    Proveedores {
        termino: String,
    },
    GuardarProveedor {
        proveedor: NuevoProveedorDto,
    },
    Pedidos {
        limite: usize,
    },
    VerPedido {
        id: i64,
    },
    NuevoPedido {
        proveedor_id: i64,
        nota: String,
    },
    /// `codigo` es SKU o código de barras. Si el producto ya está, se suma.
    AgregarAPedido {
        pedido_id: i64,
        codigo: String,
        qty_millis: i64,
        /// `None` = el costo que ya tiene el producto.
        costo_cents: Option<i64>,
    },
    QuitarDePedido {
        pedido_id: i64,
        product_id: i64,
    },
    RecibirPedido {
        id: i64,
        entregas: Vec<EntregaDto>,
    },
    CancelarPedido {
        id: i64,
    },

    // --- impresora ---
    /// Busca impresoras conectadas y dice qué son.
    Impresoras,
    /// Manda un ticket de prueba para comprobar el perfil y el cable.
    ProbarImpresion {
        ruta: String,
        baudios: i64,
        perfil: String,
        /// ¿Abrir el cajón también? Así se prueba el RJ11 de una vez.
        abrir_cajon: bool,
    },
}

/// Cuerpo de una respuesta exitosa. Un enum para que el contrato sea cerrado y
/// el compilador te obligue a manejar todos los casos.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Cuerpo {
    Vacio,
    Usuarios(Vec<UsuarioDto>),
    Sesion(SesionDto),
    Productos(Vec<ProductoDto>),
    Venta(VentaGuardadaDto),
    Caja(EstadoCajaDto),
    Clientes(Vec<ClienteDto>),
    Cliente(ClienteDto),
    Totales(TotalesDto),
    Top(Vec<TopProductoDto>),
    Utilidad(Vec<UtilidadProductoDto>),
    Ventas(Vec<VentaResumenDto>),
    Configuracion(ConfigDto),
    Impresoras(Vec<ImpresoraDto>),
    Proveedores(Vec<ProveedorDto>),
    Pedido(Box<PedidoDto>),
    Pedidos(Vec<PedidoDto>),
    Prueba(PruebaImpresionDto),
    Importe(ResultadoImporte),
    Ticket(TicketDto),
    Cajeros(Vec<CajeroDto>),
    Categorias(Vec<CategoriaDto>),
    Respaldos(EstadoRespaldosDto),
    Unidades(Vec<UnidadDto>),
    Auditoria(EventosAuditoriaDto),
    AnalisisCsv(AnalisisCsvDto),
    Conteo(ConteoDto),
    Paquete(PaqueteDto),
    EstadoActualizacion(EstadoActualizacionDto),
    /// En caja: son los cuerpos más grandes y la respuesta se copia en cada
    /// llamada (incluidas las que van por red al móvil).
    ConfigFiscal(Box<ConfigFiscalDto>),
    Factura(Box<FacturaDto>),
    Facturas(Vec<FacturaDto>),
    /// Respuesta libre (folio, id, saldo formateado, CSV...).
    Texto(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Respuesta {
    Ok(Cuerpo),
    /// Mensaje listo para mostrarse al cajero tal cual.
    Error(String),
}

impl Respuesta {
    pub fn es_ok(&self) -> bool {
        matches!(self, Respuesta::Ok(_))
    }

    pub fn es_error(&self) -> bool {
        matches!(self, Respuesta::Error(_))
    }

    pub fn vacio() -> Respuesta {
        Respuesta::Ok(Cuerpo::Vacio)
    }

    pub fn error(mensaje: impl Into<String>) -> Respuesta {
        Respuesta::Error(mensaje.into())
    }
}

/// La única forma de hablar con el punto de venta.
///
/// - [`crate::LocalApi`] lo implementa en proceso (la app de escritorio).
/// - [`crate::HttpApi`] lo implementa por red (app móvil, otra PC, pruebas).
///
/// La interfaz sólo conoce este trait: cambiar el motor gráfico no requiere
/// tocar ninguna implementación.
pub trait PosApi: Send {
    fn ejecutar(&mut self, peticion: Peticion) -> Respuesta;

    // ------------------------------------------------ ayudantes tipados ----

    /// La lista pública, para armar la pantalla de login.
    fn usuarios_login(&mut self) -> Result<Vec<UsuarioDto>, String> {
        extraer_usuarios(self.ejecutar(Peticion {
            token: None,
            comando: Comando::UsuariosLogin,
        }))
    }

    /// La lista completa. Pide haber iniciado sesión con `Permiso::Usuarios`.
    fn usuarios(&mut self, token: Option<String>) -> Result<Vec<UsuarioDto>, String> {
        extraer_usuarios(self.ejecutar(Peticion {
            token,
            comando: Comando::Usuarios,
        }))
    }

    fn login(&mut self, username: &str, pin: &str) -> Result<SesionDto, String> {
        match self.ejecutar(Peticion {
            token: None,
            comando: Comando::Login {
                username: username.to_string(),
                pin: pin.to_string(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Sesion(s)) => Ok(s),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al iniciar sesión".into()),
        }
    }

    fn buscar(
        &mut self,
        token: &str,
        termino: &str,
        limite: usize,
    ) -> Result<Vec<ProductoDto>, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::BuscarProductos {
                termino: termino.to_string(),
                limite,
            },
        }) {
            Respuesta::Ok(Cuerpo::Productos(p)) => Ok(p),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al buscar productos".into()),
        }
    }

    fn estado_caja(&mut self, token: &str) -> Result<EstadoCajaDto, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::EstadoCaja,
        }) {
            Respuesta::Ok(Cuerpo::Caja(c)) => Ok(c),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al leer la caja".into()),
        }
    }

    fn cobrar(&mut self, token: &str, venta: VentaDto) -> Result<VentaGuardadaDto, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::Cobrar { venta },
        }) {
            Respuesta::Ok(Cuerpo::Venta(v)) => Ok(v),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al cobrar".into()),
        }
    }

    fn clientes(&mut self, token: &str, termino: &str) -> Result<Vec<ClienteDto>, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::Clientes {
                termino: termino.to_string(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Clientes(c)) => Ok(c),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al buscar clientes".into()),
        }
    }

    /// Importa un CSV y devuelve el reporte (creados, actualizados, errores).
    fn importar_csv(
        &mut self,
        token: &str,
        texto: &str,
        actualizar: bool,
    ) -> Result<ResultadoImporte, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::ImportarCsv {
                texto: texto.to_string(),
                actualizar,
                mapeo: Vec::new(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Importe(r)) => Ok(r),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al importar".into()),
        }
    }

    fn ticket_por_folio(&mut self, token: &str, folio: &str) -> Result<TicketDto, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::TicketPorFolio {
                folio: folio.to_string(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Ticket(t)) => Ok(t),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al leer el ticket".into()),
        }
    }

    fn totales_por_cajero(&mut self, token: &str) -> Result<Vec<CajeroDto>, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::TotalesPorCajero,
        }) {
            Respuesta::Ok(Cuerpo::Cajeros(c)) => Ok(c),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al leer las ventas por cajero".into()),
        }
    }

    fn categorias(&mut self, token: &str) -> Result<Vec<CategoriaDto>, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::Categorias,
        }) {
            Respuesta::Ok(Cuerpo::Categorias(c)) => Ok(c),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al leer las categorías".into()),
        }
    }

    /// El catálogo como texto CSV.
    fn exportar_csv(&mut self, token: &str) -> Result<String, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::ExportarCsv,
        }) {
            Respuesta::Ok(Cuerpo::Texto(t)) => Ok(t),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al exportar".into()),
        }
    }

    fn totales_hoy(&mut self, token: &str) -> Result<TotalesDto, String> {
        match self.ejecutar(Peticion {
            token: Some(token.to_string()),
            comando: Comando::TotalesHoy,
        }) {
            Respuesta::Ok(Cuerpo::Totales(t)) => Ok(t),
            Respuesta::Error(e) => Err(e),
            _ => Err("Respuesta inesperada al leer los totales".into()),
        }
    }
}

fn extraer_usuarios(r: Respuesta) -> Result<Vec<UsuarioDto>, String> {
    match r {
        Respuesta::Ok(Cuerpo::Usuarios(u)) => Ok(u),
        Respuesta::Error(e) => Err(e),
        _ => Err("Respuesta inesperada al listar usuarios".into()),
    }
}

/// Igual que `PosApi` pero entre hilos: el servidor LAN lo comparte con un
/// `Arc<Mutex<..>>`.
pub type ApiCompartida = std::sync::Arc<std::sync::Mutex<Box<dyn PosApi>>>;
