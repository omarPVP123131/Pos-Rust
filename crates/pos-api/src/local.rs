//! Implementación en proceso: la que usa la app de escritorio.
//!
//! Aquí está **toda** la orquestación del negocio. La interfaz no sabe cómo se
//! descuenta el inventario: sólo llama. Nada pide PIN de encargado; queda
//! escrito quién lo hizo.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pos_core::{
    Arqueo, Money, Payment, PaymentMethod, Permiso, Quantity, Role, Sale, SaleLine, TaxRate,
};
use pos_db::auditoria;
use pos_db::cash::Cash;
use pos_db::categorias::{asignar, Categories};
use pos_db::clientes::{Clientes, NuevoCliente};
use pos_db::conteo;
use pos_db::pedidos;
use pos_db::products::{NewProduct, ProductUpdate, Products};
use pos_db::respaldo;
use pos_db::sales::{FilaVenta, Sales};
use pos_db::settings::Settings;
use pos_db::users::Users;
use pos_db::Database;

use crate::api::{Comando, Cuerpo, Peticion, PosApi, Respuesta};
use crate::dto::*;
use crate::fiscal;
use crate::importar::{analizar, exportar, interpretar_con, Columna, ErrorRenglon};

pub struct LocalApi {
    db: Database,
    /// token -> id de usuario
    tokens: HashMap<String, i64>,
    /// De dónde sacar el ejecutable a actualizar. Si nadie lo dice, se usa el
    /// que está corriendo; las pruebas ponen uno de mentiritas para no
    /// reemplazar el binario de pruebas.
    ejecutable: Option<PathBuf>,
}

impl LocalApi {
    pub fn new(db: Database) -> Self {
        LocalApi {
            db,
            tokens: HashMap::new(),
            ejecutable: None,
        }
    }

    /// Para las pruebas y para instalaciones raras: fija qué archivo se
    /// actualiza en lugar del ejecutable en curso.
    pub fn con_ejecutable(mut self, ruta: impl Into<PathBuf>) -> Self {
        self.ejecutable = Some(ruta.into());
        self
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    // ------------------------------------------------------------ respaldos --

    /// Carpeta configurada; si no hay ninguna, la sugerida junto a la base.
    pub fn carpeta_respaldo(&self) -> PathBuf {
        let guardada = Settings::get_or(self.db.conn(), "backup_folder", "");
        if guardada.trim().is_empty() {
            respaldo::carpeta_sugerida(self.db.ruta())
        } else {
            PathBuf::from(guardada.trim())
        }
    }

    fn cfg_i64(&self, clave: &str, defecto: i64) -> i64 {
        Settings::as_i64(self.db.conn(), clave, defecto)
    }

    fn cfg_bool(&self, clave: &str, defecto: bool) -> bool {
        let d = if defecto { "1" } else { "0" };
        Settings::get_or(self.db.conn(), clave, d) == "1"
    }

    /// El estado tal como lo ve la interfaz.
    pub fn estado_respaldos(&self) -> EstadoRespaldosDto {
        let carpeta = self.carpeta_respaldo();
        let cada_dias = self.cfg_i64("backup_every_days", 1);
        let e = respaldo::estado(&carpeta, cada_dias, respaldo::ahora());
        EstadoRespaldosDto {
            carpeta: e.carpeta,
            total: e.total,
            bytes: e.bytes,
            ultimo: e.ultimo.map(RespaldoDto::from),
            pendiente: e.pendiente,
            automatico: self.cfg_bool("backup_enabled", true),
            cada_dias,
            conservar: self.cfg_i64("backup_keep", 14),
            ultimo_verificado: None,
            mensaje: String::new(),
            recientes: respaldo::listar(&carpeta)
                .into_iter()
                .take(10)
                .map(RespaldoDto::from)
                .collect(),
        }
    }

    /// Pone la categoría de cada renglón importado. Las que no existen se
    /// crean al vuelo: nadie quiere capturar 30 categorías antes de importar.
    fn asignar_categorias_importadas(
        &self,
        productos: &[pos_db::products::NewProduct],
        categorias: &[Option<String>],
    ) -> Result<(), String> {
        if !categorias.iter().any(|c| c.is_some()) {
            return Ok(());
        }
        // Un id por nombre: "Abarrotes" se crea una sola vez, aunque venga
        // en 200 renglones.
        let mut ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for nombre in categorias.iter().flatten() {
            let nombre = nombre.trim();
            if nombre.is_empty() || ids.contains_key(nombre) {
                continue;
            }
            let id = Categories::asegurar(self.db.conn(), nombre).map_err(|e| e.to_string())?;
            ids.insert(nombre.to_string(), id);
        }
        // El catálogo de un solo golpe, para no buscar por SKU renglón por
        // renglón: importar 10,000 productos debe seguir siendo rápido.
        let existentes =
            Products::list_active(self.db.conn(), usize::MAX / 4).map_err(|e| e.to_string())?;
        let por_sku: std::collections::HashMap<&str, i64> =
            existentes.iter().map(|p| (p.sku.as_str(), p.id)).collect();
        for (producto, categoria) in productos.iter().zip(categorias.iter()) {
            let Some(nombre) = categoria
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
            else {
                continue;
            };
            let (Some(categoria_id), Some(producto_id)) =
                (ids.get(nombre), por_sku.get(producto.sku.as_str()))
            else {
                continue;
            };
            asignar(self.db.conn(), *producto_id, Some(*categoria_id))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    // ------------------------------------------------- actualizaciones --

    /// El ejecutable que se actualiza: el de las pruebas o el que corre.
    fn ejecutable(&self) -> Result<PathBuf, String> {
        match &self.ejecutable {
            Some(r) => Ok(r.clone()),
            None => std::env::current_exe()
                .map_err(|e| format!("No pude saber dónde está el programa instalado: {e}")),
        }
    }

    fn estado_actualizacion(&self) -> EstadoActualizacionDto {
        match self.ejecutable() {
            Ok(exe) => {
                let e = pos_update::aplicar::estado(&exe);
                EstadoActualizacionDto {
                    version_instalada: e.version,
                    ejecutable: e.ejecutable,
                    hay_respaldo: e.hay_respaldo,
                    hay_pendiente: e.hay_pendiente,
                    version_respaldo: e.version_respaldo,
                }
            }
            Err(_) => EstadoActualizacionDto::default(),
        }
    }

    /// Lee el paquete y reporta qué trae. **No instala nada.**
    fn revisar_actualizacion(&self, ruta: &str) -> PaqueteDto {
        let mut dto = PaqueteDto {
            ruta: ruta.to_string(),
            ..PaqueteDto::default()
        };
        let bytes = match std::fs::read(ruta) {
            Ok(b) => b,
            Err(e) => {
                dto.motivo = format!("No pude leer {ruta}: {e}");
                return dto;
            }
        };
        match pos_update::paquete::revisar(&bytes, pos_update::VERSION) {
            Ok(p) => {
                dto.valido = true;
                dto.version = p.manifiesto.version.clone();
                dto.notas = p.manifiesto.notas.clone();
                dto.tamanio = p.manifiesto.tamanio;
                dto.huella = pos_update::aplicar::huella(&p.payload);
            }
            // Un paquete inválido no es un error de la API: es información
            // que el encargado necesita ver en pantalla.
            Err(e) => dto.motivo = e.to_string(),
        }
        dto
    }

    /// Instala el paquete. El trabajo pesado (respaldo, cambio, prueba y
    /// marcha atrás) vive en `pos-update`; aquí sólo se anota en la bitácora.
    fn aplicar_actualizacion(&mut self, ruta: &str, uid: Option<i64>) -> Result<String, String> {
        let exe = self.ejecutable()?;
        // Veinte segundos: en un Celeron con disco lento arrancar tarda, pero
        // no veinte segundos. Si tarda más, algo anda mal y mejor se deshace.
        let probar = |exe: &std::path::Path| pos_update::aplicar::probar_arranque(exe, 20);
        let resultado = pos_update::aplicar::aplicar(std::path::Path::new(ruta), &exe, probar)
            .map_err(|e| e.to_string())?;
        let texto = if resultado.deshecho {
            format!(
                "No se pudo instalar la {}. Se dejó la versión que tenías (la caja no quedó a medias). Motivo: {}",
                resultado.version, resultado.motivo
            )
        } else {
            format!(
                "Actualizado a la {}. Cierra y abre el programa para que entre lo nuevo. \
                 La anterior quedó guardada: si no arranca, se deshace sola.",
                resultado.version
            )
        };
        auditoria::anotar(
            self.db.conn(),
            uid,
            if resultado.deshecho {
                "actualizacion_deshecha"
            } else {
                "actualizacion"
            },
            "programa",
            None,
            &format!(
                "{} {}{}",
                resultado.version,
                if resultado.deshecho {
                    "deshecha: "
                } else {
                    "instalada"
                },
                if resultado.deshecho {
                    resultado.motivo.clone()
                } else {
                    String::new()
                }
            ),
        );
        Ok(texto)
    }

    /// Arma el estado completo de un conteo para la interfaz.
    fn conteo_dto(&self, id: i64) -> Result<Cuerpo, String> {
        let c = conteo::por_id(self.db.conn(), id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("No existe el conteo {id}"))?;
        let renglones = conteo::renglones(self.db.conn(), id).map_err(|e| e.to_string())?;
        let resumen = conteo::resumen(self.db.conn(), id).map_err(|e| e.to_string())?;
        Ok(Cuerpo::Conteo(ConteoDto {
            id: c.id,
            estado: c.estado,
            alcance: c.alcance,
            filtro: c.filtro,
            created_at: c.created_at,
            renglones: renglones
                .into_iter()
                .map(|r| {
                    // Se calcula antes de mover los campos.
                    let diferencia_millis = r.diferencia_millis();
                    RenglonConteoDto {
                        product_id: r.product_id,
                        sku: r.sku,
                        nombre: r.nombre,
                        unidad: r.unidad,
                        categoria: r.categoria,
                        esperado_millis: r.esperado_millis,
                        contado_millis: r.contado_millis,
                        actual_millis: r.actual_millis,
                        diferencia_millis,
                    }
                })
                .collect(),
            productos: resumen.productos,
            contados: resumen.contados,
            con_diferencia: resumen.con_diferencia,
            faltantes_millis: resumen.faltantes_millis,
            sobrantes_millis: resumen.sobrantes_millis,
            valor_merma_cents: resumen.valor_merma_cents,
        }))
    }

    /// El recibo del conteo: lo que se imprime al terminar.
    fn texto_conteo(&self, r: &conteo::ResumenConteo, aplicado: bool) -> String {
        format!(
            "Conteo terminado.\n\
             Productos contados: {} de {}\n\
             Con diferencia: {}\n\
             Faltantes: {:.3} unidades\n\
             Sobrantes: {:.3} unidades\n\
             Merma valuada a costo: ${:.2}\n\
             {}",
            r.contados,
            r.productos,
            r.con_diferencia,
            r.faltantes_millis as f64 / 1000.0,
            r.sobrantes_millis as f64 / 1000.0,
            r.valor_merma_cents as f64 / 100.0,
            if aplicado {
                "Se ajustó el inventario."
            } else {
                "Sin aplicar: el inventario sigue igual."
            }
        )
    }

    /// Hace el respaldo y rota los viejos. `forzar` salta el "¿ya toca?".
    pub fn respaldar(&self, forzar: bool) -> Result<String, String> {
        let carpeta = self.carpeta_respaldo();
        let cada_dias = self.cfg_i64("backup_every_days", 1);
        if !forzar && !respaldo::necesita(&carpeta, cada_dias, respaldo::ahora()) {
            return Err("Todavía no toca respaldar".to_string());
        }
        let info = respaldo::crear(&self.db, &carpeta).map_err(|e| e.to_string())?;
        let borrados = respaldo::rotar(&carpeta, self.cfg_i64("backup_keep", 14) as usize);
        // Queda anotado: el día que falte dinero, la bitácora dice si había
        // respaldo reciente o no.
        auditoria::anotar(
            self.db.conn(),
            None,
            "respaldo",
            "system",
            None,
            &format!(
                "{} ({} bytes){}{}",
                info.nombre,
                info.bytes,
                if forzar { " [al cortar]" } else { "" },
                if borrados.is_empty() {
                    String::new()
                } else {
                    format!(" — se borraron {} viejos", borrados.len())
                }
            ),
        );
        Ok(if borrados.is_empty() {
            format!("Respaldo listo: {}", info.nombre)
        } else {
            format!(
                "Respaldo listo: {} (se borraron {} viejos)",
                info.nombre,
                borrados.len()
            )
        })
    }

    /// Se llama desde el lazo principal (cada minuto) y al arrancar.
    /// Hace el respaldo **si está activo y si ya toca**. Nunca truena: un
    /// respaldo que falla no puede tumbar el punto de venta.
    pub fn tick_respaldo(&self) -> Option<String> {
        if !self.cfg_bool("backup_enabled", true) {
            return None;
        }
        match self.respaldar(false) {
            Ok(m) => Some(m),
            Err(e) if e == "Todavía no toca respaldar" => None,
            Err(e) => Some(format!("No se pudo hacer el respaldo automático: {e}")),
        }
    }

    fn nuevo_token(&self) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{nanos:x}-{:x}", self.tokens.len())
    }

    fn uid(&self, token: &Option<String>) -> Result<i64, String> {
        let t = token.as_ref().ok_or("Primero inicia sesión")?;
        self.tokens
            .get(t)
            .copied()
            .ok_or_else(|| "Sesión vencida. Vuelve a entrar.".to_string())
    }

    /// Exige el permiso al usuario **de la sesión** (no pide PIN extra).
    fn exigir_sesion(&self, uid: i64, permiso: Permiso) -> Result<(), String> {
        let usuario = Users::by_id(self.db.conn(), uid)
            .map_err(|e| e.to_string())?
            .ok_or("Usuario inexistente")?;
        if usuario.puede(permiso) {
            Ok(())
        } else {
            Err(format!(
                "{} no tiene permiso para {}",
                usuario.display_name, permiso
            ))
        }
    }

    /// Cómo se llama quien está cobrando. Si no aparece, "cajero".
    fn nombre_de(&self, uid: i64) -> String {
        Users::by_id(self.db.conn(), uid)
            .ok()
            .flatten()
            .map(|u| u.display_name)
            .unwrap_or_else(|| "cajero".into())
    }

    /// Anota lo que hizo el cajero. Nunca falla: si no se puede escribir la
    /// bitácora, la operación sigue (lo otro sería peor).
    fn anotar_hecho(&self, uid: i64, accion: &str, entidad: &str, detalle: &str) {
        auditoria::anotar(self.db.conn(), Some(uid), accion, entidad, None, detalle);
    }

    // ------------------------------------------------------------- comandos

    /// Sólo lo que la pantalla de login necesita: nombre de usuario y cómo se
    /// llama cada quién. Nada de roles, ni de quién está dado de alta.
    fn lista_para_login(&self) -> Result<Cuerpo, String> {
        let lista = Users::list(self.db.conn()).map_err(|e| e.to_string())?;
        Ok(Cuerpo::Usuarios(
            lista
                .into_iter()
                .filter(|u| u.is_active)
                .map(|u| UsuarioDto {
                    id: u.id,
                    username: u.username,
                    display_name: u.display_name,
                    role: String::new(),
                    is_active: u.is_active,
                })
                .collect(),
        ))
    }

    fn lista_usuarios(&self) -> Result<Cuerpo, String> {
        let lista = Users::list(self.db.conn()).map_err(|e| e.to_string())?;
        Ok(Cuerpo::Usuarios(
            lista
                .into_iter()
                .map(|u| UsuarioDto {
                    id: u.id,
                    username: u.username,
                    display_name: u.display_name,
                    role: u.role.as_str().to_string(),
                    is_active: u.is_active,
                })
                .collect(),
        ))
    }

    fn crear_usuario(
        &mut self,
        quien: i64,
        username: &str,
        display_name: &str,
        pin: &str,
        role: &str,
    ) -> Result<Cuerpo, String> {
        let user = username.trim();
        let nombre = display_name.trim();
        if user.chars().count() < 2 {
            return Err("El usuario tiene que tener al menos dos letras.".into());
        }
        if nombre.chars().count() < 2 {
            return Err("El nombre tiene que tener al menos dos letras.".into());
        }
        let rol = Role::desde_texto(role);
        Users::crear(self.db.conn_mut(), user, nombre, pin, rol).map_err(|e| e.to_string())?;
        self.anotar_hecho(
            quien,
            "usuario",
            "user",
            &format!("alta de {user} ({})", rol.label()),
        );
        Ok(Cuerpo::Vacio)
    }

    fn actualizar_usuario(
        &self,
        quien: i64,
        id: i64,
        display_name: &str,
        role: &str,
    ) -> Result<Cuerpo, String> {
        let rol = Role::desde_texto(role);
        let actual = Users::by_id(self.db.conn(), id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Ese usuario no existe.".to_string())?;
        if actual.role == Role::Admin && rol != Role::Admin {
            let n = Users::admins_activos(self.db.conn()).map_err(|e| e.to_string())?;
            if n <= 1 {
                return Err("No puedes dejar la tienda sin un administrador.".into());
            }
        }
        Users::actualizar(self.db.conn(), id, display_name, rol).map_err(|e| e.to_string())?;
        self.anotar_hecho(
            quien,
            "usuario",
            "user",
            &format!("editó a {} ({})", display_name.trim(), rol.label()),
        );
        Ok(Cuerpo::Vacio)
    }

    fn cambiar_pin_usuario(&self, quien: i64, id: i64, pin: &str) -> Result<Cuerpo, String> {
        Users::cambiar_pin(self.db.conn(), id, pin).map_err(|e| e.to_string())?;
        self.anotar_hecho(quien, "usuario", "user", "cambió el PIN de alguien");
        Ok(Cuerpo::Vacio)
    }

    fn desactivar_usuario(&self, quien: i64, id: i64) -> Result<Cuerpo, String> {
        if id == quien {
            return Err("No puedes darte de baja a ti mismo.".into());
        }
        let actual = Users::by_id(self.db.conn(), id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Ese usuario no existe.".to_string())?;
        if actual.role == Role::Admin {
            let n = Users::admins_activos(self.db.conn()).map_err(|e| e.to_string())?;
            if n <= 1 {
                return Err("No puedes dejar la tienda sin un administrador.".into());
            }
        }
        Users::desactivar(self.db.conn(), id).map_err(|e| e.to_string())?;
        self.anotar_hecho(
            quien,
            "usuario",
            "user",
            &format!("dio de baja a {}", actual.username),
        );
        Ok(Cuerpo::Vacio)
    }

    fn iniciar_sesion(&mut self, username: &str, pin: &str) -> Result<Cuerpo, String> {
        let usuario =
            Users::autenticar(self.db.conn_mut(), username, pin).map_err(|e| e.to_string())?;
        let token = self.nuevo_token();
        self.tokens.insert(token.clone(), usuario.id);
        Ok(Cuerpo::Sesion(SesionDto {
            usuario: UsuarioDto {
                id: usuario.id,
                username: usuario.username,
                display_name: usuario.display_name,
                role: usuario.role.as_str().to_string(),
                is_active: usuario.is_active,
            },
            token,
        }))
    }

    fn productos(&self, lista: Vec<pos_db::products::Product>) -> Cuerpo {
        Cuerpo::Productos(
            lista
                .into_iter()
                .map(|p| ProductoDto {
                    id: p.id,
                    sku: p.sku,
                    name: p.name,
                    price_cents: p.price_cents,
                    cost_cents: p.cost_cents,
                    tax_bp: p.tax_bp,
                    stock_millis: p.stock_millis,
                    min_stock_millis: p.min_stock_millis,
                    track_inventory: p.track_inventory,
                    clave_sat: "01010101".into(),
                    clave_unidad: "H87".into(),
                    unidad_sat: "Pieza".into(),
                })
                .collect(),
        )
    }

    fn realizar_cobro(&mut self, uid: i64, dto: VentaDto) -> Result<Cuerpo, String> {
        if dto.lineas.is_empty() {
            return Err("La venta no tiene productos".into());
        }

        // Se arma la venta contra el catálogo real: la interfaz nunca fija
        // precios por su cuenta sin que aquí se valide.
        let mut sale = Sale::new("");
        sale.customer_id = dto.cliente_id;
        sale.note = dto.nota.clone();

        let mut precio_manual = false;
        let mut con_descuento = dto.descuento_global_cents > 0;
        let mut pedido: HashMap<i64, (String, i64, i64)> = HashMap::new();

        for linea in &dto.lineas {
            let producto = Products::by_sku(self.db.conn(), &linea.sku)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("No existe el producto '{}'", linea.sku))?;

            // `precio_cents = 0` significa "usa el precio del catálogo", no
            // "regálalo". Antes se comparaba en crudo y cualquier cliente que
            // mandara 0 (la app móvil, por ejemplo) provocaba que una venta
            // normal pidiera autorización de supervisor... o, peor, que se
            // cobrara a $0.00.
            let precio_final = if linea.precio_cents > 0 {
                linea.precio_cents
            } else {
                producto.price_cents
            };
            if precio_final != producto.price_cents {
                precio_manual = true;
            }
            if linea.descuento_cents > 0 {
                con_descuento = true;
            }

            let qty = Quantity::from_millis(linea.qty_millis);
            if qty.millis() <= 0 {
                return Err("Hay una línea con cantidad en cero".into());
            }
            if producto.track_inventory {
                let entrada = pedido.entry(producto.id).or_insert((
                    producto.name.clone(),
                    producto.stock_millis,
                    0,
                ));
                entrada.2 += qty.millis();
            }
            let descuento = Money::from_cents(linea.descuento_cents);
            let bruto = Money::from_cents(producto.price_cents).mul_qty(qty);
            if descuento > bruto {
                return Err(format!(
                    "El descuento de '{}' es mayor que su importe",
                    producto.name
                ));
            }

            sale.add_line(SaleLine {
                product_id: Some(producto.id),
                sku: producto.sku.clone(),
                description: producto.name.clone(),
                qty,
                unit_price: Money::from_cents(precio_final),
                discount: descuento,
                tax: TaxRate::from_bp(producto.tax_bp),
                unit_cost: Money::from_cents(producto.cost_cents),
            })
            .map_err(|e| e.to_string())?;
        }

        // 29 en anaquel y 30 en el ticket: se dice ahora, no después de
        // cobrar. Si hay dos renglones del mismo producto, se suman.
        for (nombre, hay, pide) in pedido.values() {
            if pide > hay {
                return Err(format!(
                    "No hay suficiente '{nombre}': hay {}, pides {}",
                    Quantity::from_millis(*hay).format(),
                    Quantity::from_millis(*pide).format()
                ));
            }
        }

        let fiado = dto.pagos.iter().any(|p| p.metodo == "credito");

        // Nada pide PIN de encargado: el cajero cobra y queda escrito
        // quién lo hizo, de cuánto a cuánto, a quién se fió.
        if precio_manual {
            let cajero = self.nombre_de(uid);
            let cambios: Vec<String> = dto
                .lineas
                .iter()
                .filter_map(|l| {
                    if l.precio_cents <= 0 {
                        return None;
                    }
                    let lista = Products::by_sku(self.db.conn(), &l.sku)
                        .ok()
                        .flatten()
                        .map(|p| p.price_cents)
                        .unwrap_or(0);
                    if l.precio_cents == lista {
                        return None;
                    }
                    let qty = l.qty_millis / 1000;
                    Some(format!(
                        "{}: ${:.2} → ${:.2}{}",
                        l.sku,
                        lista as f64 / 100.0,
                        l.precio_cents as f64 / 100.0,
                        if qty > 1 {
                            format!(" (x{qty})")
                        } else {
                            String::new()
                        }
                    ))
                })
                .collect();
            auditoria::anotar(
                self.db.conn(),
                Some(uid),
                "precio",
                "venta",
                None,
                &format!("{cajero} cambió el precio: {}", cambios.join(", ")),
            );
        }
        if con_descuento {
            let cajero = self.nombre_de(uid);
            let detalles: Vec<String> = dto
                .lineas
                .iter()
                .filter(|l| l.descuento_cents > 0)
                .map(|l| format!("-${:.2} en {}", l.descuento_cents as f64 / 100.0, l.sku))
                .collect();
            let global = if dto.descuento_global_cents > 0 {
                format!(
                    " y -${:.2} global",
                    dto.descuento_global_cents as f64 / 100.0
                )
            } else {
                String::new()
            };
            self.anotar_hecho(
                uid,
                "descuento",
                "venta",
                &format!(
                    "{cajero} aplicó descuento: {}{}",
                    detalles.join(", "),
                    global
                ),
            );
        }
        if fiado {
            if dto.cliente_id.is_none() {
                return Err("Una venta a crédito necesita un cliente".into());
            }
            let cajero = self.nombre_de(uid);
            let nombre = dto
                .cliente_id
                .and_then(|id| Clientes::por_id(self.db.conn(), id).ok().flatten())
                .map(|c| c.name)
                .unwrap_or_else(|| format!("cliente {}", dto.cliente_id.unwrap_or(0)));
            self.anotar_hecho(
                uid,
                "fiado",
                "venta",
                &format!(
                    "{cajero} fió a {nombre} por ${:.2}",
                    sale.total().cents() as f64 / 100.0
                ),
            );
        }

        if dto.descuento_global_cents > 0 {
            sale.apply_global_discount(Money::from_cents(dto.descuento_global_cents))
                .map_err(|e| e.to_string())?;
        }

        for pago in &dto.pagos {
            let metodo = metodo_desde_str(&pago.metodo)?;
            let mut p = Payment::new(metodo, Money::from_cents(pago.monto_cents));
            if metodo == PaymentMethod::Efectivo && pago.recibido_cents > 0 {
                p.received = Money::from_cents(pago.recibido_cents);
            }
            sale.add_payment(p);
        }

        if !sale.is_fully_paid() {
            return Err(format!("Falta por cubrir {}", sale.balance_due()));
        }

        let sesion = Cash::abierta(self.db.conn())
            .map_err(|e| e.to_string())?
            .map(|s| s.id)
            .ok_or("No hay caja abierta. Ábrela antes de vender.")?;

        let cambio = sale.change_due();
        Sales::save(self.db.conn_mut(), &mut sale, uid, Some(sesion)).map_err(|e| e.to_string())?;

        Ok(Cuerpo::Venta(VentaGuardadaDto {
            id: sale.id.unwrap_or(0),
            folio: sale.folio.clone(),
            subtotal_cents: sale.subtotal().cents(),
            impuesto_cents: sale.tax_total().cents(),
            descuento_cents: sale.discount_total().cents(),
            total_cents: sale.total().cents(),
            cambio_cents: cambio.cents(),
        }))
    }

    fn lectura_caja(&self) -> Result<Cuerpo, String> {
        let abierta = Cash::abierta(self.db.conn()).map_err(|e| e.to_string())?;
        let Some(sesion) = abierta else {
            return Ok(Cuerpo::Caja(EstadoCajaDto {
                abierta: false,
                ..Default::default()
            }));
        };
        let corte = Cash::corte_actual(self.db.conn(), sesion.id).map_err(|e| e.to_string())?;
        let (tickets, vendido) =
            Cash::ventas_de_sesion(self.db.conn(), sesion.id).map_err(|e| e.to_string())?;

        Ok(Cuerpo::Caja(EstadoCajaDto {
            abierta: true,
            sesion_id: Some(sesion.id),
            fondo_cents: corte.fondo_inicial.cents(),
            ventas_efectivo_cents: corte.ventas_efectivo.cents(),
            entradas_cents: corte.entradas.cents(),
            retiros_cents: corte.retiros.cents(),
            gastos_cents: corte.gastos.cents(),
            esperado_cents: corte.esperado().cents(),
            tickets,
            vendido_cents: vendido,
        }))
    }

    fn lista_clientes(&self, termino: &str) -> Result<Cuerpo, String> {
        let lista = Clientes::buscar(self.db.conn(), termino, 50).map_err(|e| e.to_string())?;
        Ok(Cuerpo::Clientes(
            lista
                .into_iter()
                .map(|c| {
                    let disponible = c.disponible().cents();
                    ClienteDto {
                        id: c.id,
                        name: c.name,
                        phone: c.phone,
                        rfc: c.rfc,
                        limite_cents: c.credit_limit_cents,
                        saldo_cents: c.balance_cents,
                        disponible_cents: disponible,
                        regimen_fiscal: c.regimen_fiscal,
                        codigo_postal: c.codigo_postal,
                        uso_cfdi: c.uso_cfdi,
                    }
                })
                .collect(),
        ))
    }
}

fn metodo_desde_str(s: &str) -> Result<PaymentMethod, String> {
    match s.trim().to_lowercase().as_str() {
        "efectivo" => Ok(PaymentMethod::Efectivo),
        "tarjeta" => Ok(PaymentMethod::Tarjeta),
        "transferencia" => Ok(PaymentMethod::Transferencia),
        "vales" => Ok(PaymentMethod::Vales),
        "credito" | "crédito" => Ok(PaymentMethod::Credito),
        _ => Err(format!("Método de pago desconocido: {s}")),
    }
}

/// Guarda las claves del SAT del producto, si el formulario trajo alguna.
///
/// `None` significa "déjalas como están": dar de alta un producto no debería
/// borrarle la clave a quien ya la tenía puesta.
fn guardar_claves_sat(
    conn: &pos_db::rusqlite::Connection,
    producto_id: i64,
    clave_sat: Option<&str>,
    clave_unidad: Option<&str>,
    unidad_sat: Option<&str>,
) {
    let (clave_sat, clave_unidad, unidad_sat) = match (
        clave_sat.map(str::trim).filter(|s| !s.is_empty()),
        clave_unidad.map(str::trim).filter(|s| !s.is_empty()),
        unidad_sat.map(str::trim).filter(|s| !s.is_empty()),
    ) {
        (None, None, None) => return,
        (a, b, c) => (
            a.unwrap_or(pos_db::products::CLAVE_SAT_GENERICA)
                .to_string(),
            b.unwrap_or(pos_db::products::CLAVE_UNIDAD).to_string(),
            c.unwrap_or("Pieza").to_string(),
        ),
    };
    let _ = Products::set_claves_sat(conn, producto_id, &clave_sat, &clave_unidad, &unidad_sat);
}

impl PosApi for LocalApi {
    fn ejecutar(&mut self, peticion: Peticion) -> Respuesta {
        // Comandos públicos: no requieren sesión.
        match &peticion.comando {
            Comando::UsuariosLogin => return a_respuesta(self.lista_para_login()),
            Comando::Login { username, pin } => {
                let u = username.clone();
                let p = pin.clone();
                return a_respuesta(self.iniciar_sesion(&u, &p));
            }
            _ => {}
        }

        let uid = match self.uid(&peticion.token) {
            Ok(u) => u,
            Err(e) => return Respuesta::error(e),
        };

        let resultado: Result<Cuerpo, String> = match peticion.comando {
            Comando::Usuarios => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Usuarios) {
                    return Respuesta::error(e);
                }
                return a_respuesta(self.lista_usuarios());
            }
            Comando::CrearUsuario {
                username,
                display_name,
                pin,
                role,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Usuarios) {
                    return Respuesta::error(e);
                }
                self.crear_usuario(uid, &username, &display_name, &pin, &role)
            }
            Comando::ActualizarUsuario {
                id,
                display_name,
                role,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Usuarios) {
                    return Respuesta::error(e);
                }
                self.actualizar_usuario(uid, id, &display_name, &role)
            }
            Comando::CambiarPinUsuario { id, pin } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Usuarios) {
                    return Respuesta::error(e);
                }
                self.cambiar_pin_usuario(uid, id, &pin)
            }
            Comando::DesactivarUsuario { id } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Usuarios) {
                    return Respuesta::error(e);
                }
                self.desactivar_usuario(uid, id)
            }
            Comando::DesbloquearUsuario { id } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Usuarios) {
                    return Respuesta::error(e);
                }
                match Users::desbloquear(self.db.conn(), id) {
                    Ok(()) => {
                        self.anotar_hecho(uid, "usuario", "user", "desbloqueó a alguien");
                        Ok(Cuerpo::Vacio)
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Comando::UsuariosLogin | Comando::Login { .. } => unreachable!(),
            Comando::Salir => {
                if let Some(t) = peticion.token {
                    self.tokens.remove(&t);
                }
                Ok(Cuerpo::Vacio)
            }

            // --- catálogo ---
            Comando::BuscarProductos { termino, limite } => {
                Products::search(self.db.conn(), &termino, limite)
                    .map(|v| self.productos(v))
                    .map_err(|e| e.to_string())
            }
            Comando::ProductoPorCodigo { codigo } => Products::by_barcode(self.db.conn(), &codigo)
                .map(|p| self.productos(p.into_iter().collect()))
                .map_err(|e| e.to_string()),
            Comando::GuardarProducto { producto } => {
                // Alta y precio: el cajero también. Importar y mermas siguen
                // pidiendo encargado.
                let categoria = match producto.categoria.as_deref().map(str::trim) {
                    Some(nombre) if !nombre.is_empty() => {
                        match Categories::asegurar(self.db.conn(), nombre) {
                            Ok(id) => Some(id),
                            Err(e) => return Respuesta::error(e.to_string()),
                        }
                    }
                    _ => None,
                };

                match producto.id {
                    // Edición: el precio y el nombre cambian, el stock no
                    // (para eso está el movimiento de inventario).
                    Some(id) => {
                        let cambios = ProductUpdate {
                            sku: producto.sku,
                            name: producto.name,
                            price_cents: producto.price_cents,
                            cost_cents: producto.cost_cents,
                            tax_bp: producto.tax_bp,
                            unit: producto.unit,
                            min_stock_millis: producto.min_stock_millis,
                            track_inventory: producto.track_inventory,
                        };
                        let resultado = Products::update(self.db.conn_mut(), id, &cambios);
                        match resultado {
                            Ok(()) => {
                                let _ =
                                    Products::set_barcodes(self.db.conn(), id, &producto.barcodes);
                                guardar_claves_sat(
                                    self.db.conn(),
                                    id,
                                    producto.clave_sat.as_deref(),
                                    producto.clave_unidad.as_deref(),
                                    producto.unidad_sat.as_deref(),
                                );
                                match asignar(self.db.conn(), id, categoria) {
                                    Ok(()) => Ok(Cuerpo::Vacio),
                                    Err(e) => Err(e.to_string()),
                                }
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    }
                    None => {
                        let nuevo = NewProduct {
                            sku: producto.sku,
                            name: producto.name,
                            price_cents: producto.price_cents,
                            cost_cents: producto.cost_cents,
                            tax_bp: producto.tax_bp,
                            unit: producto.unit,
                            stock_millis: producto.stock_millis,
                            min_stock_millis: producto.min_stock_millis,
                            track_inventory: producto.track_inventory,
                            barcodes: producto.barcodes,
                        };
                        match Products::insert(self.db.conn_mut(), &nuevo) {
                            Ok(id) => {
                                if let Some(c) = categoria {
                                    let _ = asignar(self.db.conn(), id, Some(c));
                                }
                                guardar_claves_sat(
                                    self.db.conn(),
                                    id,
                                    producto.clave_sat.as_deref(),
                                    producto.clave_unidad.as_deref(),
                                    producto.unidad_sat.as_deref(),
                                );
                                Ok(Cuerpo::Vacio)
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    }
                }
            }

            Comando::AnalizarCsv { texto } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let a = analizar(&texto);
                Ok(Cuerpo::AnalisisCsv(AnalisisCsvDto {
                    encabezados: a.encabezados,
                    sugerido: a.sugerido,
                    ejemplo: a.ejemplo,
                    separador: a.separador,
                    renglones: a.renglones,
                    tiene_nombre: a.tiene_nombre,
                    campos: Columna::campos()
                        .iter()
                        .map(|(c, e)| (c.to_string(), e.to_string()))
                        .collect(),
                }))
            }
            Comando::ImportarCsv {
                texto,
                actualizar,
                mapeo,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let mut resultado = interpretar_con(&texto, &mapeo);
                if resultado.productos.is_empty() {
                    return Respuesta::Ok(Cuerpo::Importe(resultado));
                }
                match Products::importar(self.db.conn_mut(), &resultado.productos, actualizar) {
                    Ok((creados, actualizados)) => {
                        resultado.creados = creados;
                        resultado.actualizados = actualizados;
                        // La categoría no es un campo del producto: se asigna
                        // después, cuando ya sabemos el id de cada renglón.
                        if let Err(e) = self.asignar_categorias_importadas(
                            &resultado.productos,
                            &resultado.categorias,
                        ) {
                            resultado.errores.push(ErrorRenglon {
                                renglon: 0,
                                columna: Some("categoria".into()),
                                motivo: e,
                                contenido: String::new(),
                            });
                        }
                        auditoria::anotar(
                            self.db.conn(),
                            Some(uid),
                            "importacion",
                            "product",
                            None,
                            &format!(
                                "{} creados, {} actualizados, {} con error",
                                creados,
                                actualizados,
                                resultado.errores.len()
                            ),
                        );
                        Ok(Cuerpo::Importe(resultado))
                    }
                    // El error de la base dice el renglón: se agrega al
                    // reporte en lugar de tirar todo el archivo.
                    Err(e) => {
                        resultado.errores.push(ErrorRenglon {
                            renglon: 0,
                            columna: None,
                            motivo: e.to_string(),
                            contenido: String::new(),
                        });
                        Ok(Cuerpo::Importe(resultado))
                    }
                }
            }

            Comando::TicketPorFolio { folio } => match Sales::by_folio(self.db.conn(), &folio) {
                Ok(Some(v)) => Ok(Cuerpo::Ticket(ticket_dto(&v, &self.db))),
                Ok(None) => Err(format!("No encontré el ticket {folio}")),
                Err(e) => Err(e.to_string()),
            },

            Comando::TotalesPorCajero => Sales::por_cajero(self.db.conn(), &hoy_cdmx())
                .map(|v| {
                    Cuerpo::Cajeros(
                        v.into_iter()
                            .map(|c| CajeroDto {
                                nombre: c.nombre,
                                tickets: c.tickets,
                                vendido_cents: c.vendido_cents,
                                devuelto_cents: c.devuelto_cents,
                            })
                            .collect(),
                    )
                })
                .map_err(|e| e.to_string()),

            Comando::ExportarVentasCsv { desde, hasta } => {
                let (desde, hasta) = normalizar_rango(&desde, &hasta);
                Sales::entre(self.db.conn(), &desde, &hasta, 100_000)
                    .map(|v| Cuerpo::Texto(ventas_a_csv(&v)))
                    .map_err(|e| e.to_string())
            }

            Comando::Categorias => Categories::con_conteo(self.db.conn())
                .map(|v| {
                    Cuerpo::Categorias(
                        v.into_iter()
                            .map(|(c, n)| CategoriaDto {
                                id: c.id,
                                name: c.name,
                                productos: n,
                            })
                            .collect(),
                    )
                })
                .map_err(|e| e.to_string()),

            Comando::GuardarCategoria { nombre } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                Categories::asegurar(self.db.conn(), &nombre)
                    .map(|id| Cuerpo::Texto(id.to_string()))
                    .map_err(|e| e.to_string())
            }

            Comando::ProductosBajoMinimo => Products::low_stock(self.db.conn(), 200)
                .map(|p| self.productos(p))
                .map_err(|e| e.to_string()),

            Comando::Respaldo { ruta } => {
                // Sólo quien puede configurar saca una copia: el respaldo
                // contiene costos, precios y todo el historial.
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                if ruta.trim().is_empty() {
                    return Respuesta::error("Escribe dónde guardar el respaldo".to_string());
                }
                self.db
                    .respaldar(ruta.trim())
                    .map(|_| Cuerpo::Texto(format!("Respaldo listo y verificado en {ruta}")))
                    .map_err(|e| e.to_string())
            }

            // --- facturación electrónica (CFDI 4.0) ---
            //
            // Emitir una factura no es vender: es un acto fiscal con
            // consecuencias ante el SAT. Por eso exige el mismo permiso que
            // cambiar la configuración, y por eso cada movimiento queda en la
            // bitácora con el nombre de quien lo hizo.
            Comando::ConfigFiscal => Ok(Cuerpo::ConfigFiscal(Box::new(fiscal::leer_config(
                self.db.conn(),
            )))),
            Comando::GuardarConfigFiscal { config } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                if let Some(url) = fiscal::url_http_en(&config) {
                    let quien = self.nombre_de(uid);
                    self.anotar_hecho(
                        uid,
                        "pac_http",
                        "settings",
                        &format!("{quien} intentó configurar un PAC por HTTP: {url}"),
                    );
                    return Respuesta::error(pos_fiscal::pac::mensaje_http_bloqueado(&url));
                }
                let rfc = config.rfc.trim().to_uppercase();
                if let Err(e) = fiscal::guardar_config(self.db.conn(), &config) {
                    return Respuesta::error(e);
                }
                auditoria::anotar(
                    self.db.conn(),
                    Some(uid),
                    "fiscal",
                    "settings",
                    None,
                    &format!(
                        "datos fiscales: rfc '{rfc}', regimen '{}', cp '{}'",
                        config.regimen_fiscal.trim(),
                        config.codigo_postal.trim()
                    ),
                );
                Ok(Cuerpo::Vacio)
            }
            Comando::FacturaGlobal {
                fecha,
                periodicidad,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Facturar) {
                    return Respuesta::error(e);
                }
                let f = match fiscal::factura_global(
                    self.db.conn(),
                    &fecha,
                    &periodicidad,
                    Some(uid),
                ) {
                    Ok(f) => f,
                    Err(e) => return Respuesta::error(e),
                };
                auditoria::anotar(
                    self.db.conn(),
                    Some(uid),
                    "factura_global",
                    "facturas",
                    Some(f.id),
                    &format!(
                        "folio {} del {} por {} ({} ventas)",
                        f.folio,
                        f.periodo,
                        pos_core::Money::from_cents(f.total_cents).format_mxn(),
                        f.ventas.split(',').filter(|s| !s.trim().is_empty()).count()
                    ),
                );
                Ok(Cuerpo::Factura(Box::new(f)))
            }
            Comando::Facturas { limite } => {
                fiscal::listar(self.db.conn(), limite.max(1)).map(Cuerpo::Facturas)
            }
            Comando::ReintentarFactura { id } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Facturar) {
                    return Respuesta::error(e);
                }
                if let Some(url) = fiscal::url_http_en(&fiscal::leer_config(self.db.conn())) {
                    let quien = self.nombre_de(uid);
                    self.anotar_hecho(
                        uid,
                        "pac_http",
                        "facturas",
                        &format!("{quien} intentó timbrar por HTTP: {url}"),
                    );
                    return Respuesta::error(pos_fiscal::pac::mensaje_http_bloqueado(&url));
                }
                let f = match fiscal::reintentar(self.db.conn(), id) {
                    Ok(f) => f,
                    Err(e) => return Respuesta::error(e),
                };
                if f.estado == "timbrada" {
                    // Éste sí se anota: aquí aparece el UUID que el SAT va a
                    // tener registrado.
                    auditoria::anotar(
                        self.db.conn(),
                        Some(uid),
                        "timbrado",
                        "facturas",
                        Some(id),
                        &format!(
                            "folio {} timbrada con UUID {}",
                            f.folio,
                            f.uuid.as_deref().unwrap_or("?")
                        ),
                    );
                }
                Ok(Cuerpo::Factura(Box::new(f)))
            }
            Comando::XmlFactura { id } => fiscal::xml_de(self.db.conn(), id).map(Cuerpo::Texto),
            Comando::FacturarVenta { folio, fecha } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Facturar) {
                    return Respuesta::error(e);
                }
                let f = match fiscal::facturar_venta(self.db.conn(), &folio, &fecha, Some(uid)) {
                    Ok(f) => f,
                    Err(e) => return Respuesta::error(e),
                };
                auditoria::anotar(
                    self.db.conn(),
                    Some(uid),
                    "factura_individual",
                    "facturas",
                    Some(f.id),
                    &format!(
                        "factura {} de la venta {} a {} por {}",
                        f.folio,
                        folio,
                        f.receptor_rfc,
                        pos_core::Money::from_cents(f.total_cents).format_mxn()
                    ),
                );
                Ok(Cuerpo::Factura(Box::new(f)))
            }
            Comando::VentasPorFacturar => match fiscal::ventas_por_facturar(self.db.conn()) {
                Ok(n) => Ok(Cuerpo::Texto(n.to_string())),
                Err(e) => Err(e.to_string()),
            },
            Comando::ComplementoPago {
                cliente_id,
                monto_cents,
                fecha,
                forma_pago,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Facturar) {
                    return Respuesta::error(e);
                }
                let pagado = pos_core::Money::from_cents(monto_cents);
                let f = match fiscal::complemento_pago(
                    self.db.conn(),
                    cliente_id,
                    monto_cents,
                    &fecha,
                    &forma_pago,
                    Some(uid),
                ) {
                    Ok(f) => f,
                    Err(e) => return Respuesta::error(e),
                };
                auditoria::anotar(
                    self.db.conn(),
                    Some(uid),
                    "complemento_pago",
                    "facturas",
                    Some(f.id),
                    &format!("complemento {} por {}", f.folio, pagado.format_mxn()),
                );
                Ok(Cuerpo::Factura(Box::new(f)))
            }
            Comando::SaldoPorPagar { cliente_id } => {
                match fiscal::saldo_por_pagar(self.db.conn(), cliente_id) {
                    Ok(cents) => Ok(Cuerpo::Texto(cents.to_string())),
                    Err(e) => Err(e),
                }
            }

            Comando::NotaCreditoMonto {
                folio,
                fecha,
                monto_cents,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Facturar) {
                    return Respuesta::error(e);
                }
                let monto = pos_core::Money::from_cents(monto_cents);
                let (f, _pedido) = match fiscal::nota_credito_monto(
                    self.db.conn(),
                    &folio,
                    &fecha,
                    monto_cents,
                    Some(uid),
                ) {
                    Ok(x) => x,
                    Err(e) => return Respuesta::error(e),
                };
                let cubierto = pos_core::Money::from_cents(f.total_cents);
                auditoria::anotar(
                    self.db.conn(),
                    Some(uid),
                    "nota_credito_monto",
                    "facturas",
                    Some(f.id),
                    &format!(
                        "nota {} de la venta {} por {} (se pidieron {})",
                        f.folio,
                        folio,
                        cubierto.format_mxn(),
                        monto.format_mxn()
                    ),
                );
                // La nota ya está guardada. Si las piezas no repartieron
                // exacto, NO se devuelve error: eso haría creer que no se hizo
                // cuando sí se hizo. La interfaz compara y lo dice.
                Ok(Cuerpo::Factura(Box::new(f)))
            }
            Comando::NotaCredito {
                folio,
                fecha,
                lineas,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Facturar) {
                    return Respuesta::error(e);
                }
                let pedidas: Vec<(String, i64)> = lineas
                    .iter()
                    .map(|l| (l.sku.clone(), l.cantidad_millis))
                    .collect();
                let f =
                    match fiscal::nota_credito(self.db.conn(), &folio, &fecha, &pedidas, Some(uid))
                    {
                        Ok(f) => f,
                        Err(e) => return Respuesta::error(e),
                    };
                auditoria::anotar(
                    self.db.conn(),
                    Some(uid),
                    "nota_credito",
                    "facturas",
                    Some(f.id),
                    &format!(
                        "nota {} de la venta {} por {}",
                        f.folio,
                        folio,
                        pos_core::Money::from_cents(f.total_cents).format_mxn()
                    ),
                );
                Ok(Cuerpo::Factura(Box::new(f)))
            }

            Comando::ExportarCsv => {
                let productos = match Products::list_active(self.db.conn(), 1_000_000) {
                    Ok(p) => p,
                    Err(e) => return Respuesta::error(e.to_string()),
                };
                let mut codigos = Vec::with_capacity(productos.len());
                for p in &productos {
                    let barras = Products::barcodes_de(self.db.conn(), p.id).unwrap_or_default();
                    codigos.push((p.id, barras));
                }
                Ok(Cuerpo::Texto(exportar(&productos, &codigos)))
            }

            Comando::DesactivarProducto { id } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                Products::deactivate(self.db.conn(), id)
                    .map(|_| Cuerpo::Vacio)
                    .map_err(|e| e.to_string())
            }

            Comando::AjusteInventario {
                producto_id,
                delta_millis,
                motivo,
            } => {
                if delta_millis == 0 {
                    return Respuesta::error("La cantidad no puede ser cero".to_string());
                }
                if motivo.trim().len() < 3 {
                    return Respuesta::error(
                        "Anota el motivo de la entrada o el ajuste (al menos 3 letras)".to_string(),
                    );
                }
                // Mover el inventario sin permiso es la manera más limpia de
                // tapar un faltante: si cuadra el almacén, cuadra el robo.
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                match Products::adjust_stock(
                    self.db.conn_mut(),
                    producto_id,
                    delta_millis,
                    motivo.trim(),
                    uid,
                ) {
                    Ok(()) => Ok(Cuerpo::Vacio),
                    Err(e) => Err(e.to_string()),
                }
            }

            // --- venta ---
            Comando::Cobrar { venta } => self.realizar_cobro(uid, venta),
            Comando::Devolver {
                folio,
                supervisor: _,
            } => {
                let encontrada = match Sales::by_folio(self.db.conn(), &folio) {
                    Ok(v) => v,
                    Err(e) => return Respuesta::error(e.to_string()),
                };
                match encontrada {
                    Some(v) => Sales::cancel(self.db.conn_mut(), v.id, uid)
                        .map(|_| Cuerpo::Vacio)
                        .map_err(|e| e.to_string()),
                    None => Err(format!("No existe el folio {folio}")),
                }
            }
            Comando::VentasRecientes { limite } => Sales::recent(self.db.conn(), limite)
                .map(|v| {
                    Cuerpo::Ventas(
                        v.into_iter()
                            .map(|s| VentaResumenDto {
                                id: s.id,
                                folio: s.folio,
                                created_at: s.created_at,
                                total_cents: s.total_cents,
                                status: s.status,
                            })
                            .collect(),
                    )
                })
                .map_err(|e| e.to_string()),

            // --- caja ---
            Comando::EstadoCaja => self.lectura_caja(),
            Comando::AbrirCaja { fondo_cents } => Cash::abrir(
                self.db.conn_mut(),
                uid,
                Money::from_cents(fondo_cents.max(0)),
            )
            .map(|_| Cuerpo::Vacio)
            .map_err(|e| e.to_string()),
            Comando::CerrarCaja { piezas, supervisor } => {
                let mut arqueo = Arqueo::new();
                for (denominacion, cantidad) in piezas {
                    arqueo.set_piezas(denominacion, cantidad);
                }
                let sesion = match Cash::abierta(self.db.conn()) {
                    Ok(Some(s)) => s.id,
                    Ok(None) => return Respuesta::error("No hay caja abierta"),
                    Err(e) => return Respuesta::error(e.to_string()),
                };
                let esperado = match Cash::corte_actual(self.db.conn(), sesion) {
                    Ok(c) => c.esperado(),
                    Err(e) => return Respuesta::error(e.to_string()),
                };
                let contado = arqueo.total();
                let _ = supervisor;
                // Si no cuadra, se cierra igual: queda escrito quién y cuánto.
                if contado != esperado {
                    let cajero = self.nombre_de(uid);
                    self.anotar_hecho(
                        uid,
                        "diferencia",
                        "caja",
                        &format!(
                            "{cajero} cerró caja que no cuadra (esperado {esperado}, contado {contado}, diferencia {})",
                            Money::from_cents(contado.cents() - esperado.cents())
                        ),
                    );
                }
                let r = Cash::cerrar(self.db.conn_mut(), sesion, &arqueo, uid);
                // El mejor momento para respaldar: ya cerró el día. Si falla,
                // el corte **no** se pierde: sólo queda pendiente y el lazo
                // principal lo va a reintentar.
                if r.is_ok() && self.cfg_bool("backup_on_close", true) {
                    if let Err(e) = self.respaldar(true) {
                        eprintln!("Aviso: el respaldo al cortar falló: {e}");
                    }
                }
                r.map(|_| Cuerpo::Vacio).map_err(|e| e.to_string())
            }
            Comando::MovimientoCaja {
                tipo,
                monto_cents,
                motivo,
                supervisor,
            } => {
                let movimiento = match tipo.trim().to_lowercase().as_str() {
                    "entrada" => pos_core::MovimientoCaja::Entrada,
                    "retiro" => pos_core::MovimientoCaja::Retiro,
                    "gasto" => pos_core::MovimientoCaja::Gasto,
                    otro => {
                        return Respuesta::error(format!("Tipo de movimiento inválido: {otro}"))
                    }
                };
                let _ = supervisor;
                if movimiento != pos_core::MovimientoCaja::Entrada {
                    let cajero = self.nombre_de(uid);
                    self.anotar_hecho(
                        uid,
                        "retiro",
                        "caja",
                        &format!(
                            "{cajero} registró {tipo} de ${:.2}: {motivo}",
                            monto_cents as f64 / 100.0
                        ),
                    );
                }
                let sesion = match Cash::abierta(self.db.conn()) {
                    Ok(Some(s)) => s.id,
                    Ok(None) => return Respuesta::error("No hay caja abierta"),
                    Err(e) => return Respuesta::error(e.to_string()),
                };
                Cash::movimiento(
                    self.db.conn_mut(),
                    sesion,
                    movimiento,
                    Money::from_cents(monto_cents),
                    &motivo,
                    uid,
                )
                .map(|_| Cuerpo::Vacio)
                .map_err(|e| e.to_string())
            }

            // --- clientes y cartera ---
            Comando::Clientes { termino } => self.lista_clientes(&termino),
            Comando::GuardarCliente { cliente } => {
                let nuevo = NuevoCliente {
                    name: cliente.name,
                    rfc: cliente.rfc,
                    phone: cliente.phone,
                    email: cliente.email,
                    credit_limit_cents: cliente.credit_limit_cents,
                    regimen_fiscal: cliente.regimen_fiscal,
                    codigo_postal: cliente.codigo_postal,
                    uso_cfdi: cliente.uso_cfdi,
                };
                Clientes::crear(self.db.conn_mut(), &nuevo)
                    .map(|id| Cuerpo::Texto(id.to_string()))
                    .map_err(|e| e.to_string())
            }
            Comando::Abonar {
                cliente_id,
                monto_cents,
                metodo,
            } => Clientes::abonar(
                self.db.conn_mut(),
                cliente_id,
                Money::from_cents(monto_cents),
                &metodo,
                uid,
            )
            .map(|saldo| Cuerpo::Texto(saldo.format_mxn()))
            .map_err(|e| e.to_string()),

            // --- reportes y configuración ---
            Comando::TotalesHoy => Sales::totals_today(self.db.conn())
                .map(|t| {
                    Cuerpo::Totales(TotalesDto {
                        count: t.count,
                        subtotal_cents: t.subtotal_cents,
                        tax_cents: t.tax_cents,
                        discount_cents: t.discount_cents,
                        total_cents: t.total_cents,
                        efectivo_cents: t.efectivo_cents,
                        tarjeta_cents: t.tarjeta_cents,
                        transferencia_cents: t.transferencia_cents,
                        otros_cents: t.otros_cents,
                        costo_cents: t.costo_cents,
                        utilidad_cents: t.utilidad_cents,
                    })
                })
                .map_err(|e| e.to_string()),
            Comando::TopProductos { limite } => Sales::top_products(self.db.conn(), limite)
                .map(|v| {
                    Cuerpo::Top(
                        v.into_iter()
                            .map(|(nombre, cantidad, importe)| TopProductoDto {
                                nombre,
                                cantidad_millis: cantidad,
                                importe_cents: importe,
                            })
                            .collect(),
                    )
                })
                .map_err(|e| e.to_string()),
            Comando::UtilidadHoy { limite } => Sales::utilidad_hoy(self.db.conn(), limite.max(1))
                .map(|v| {
                    Cuerpo::Utilidad(
                        v.into_iter()
                            .map(|u| UtilidadProductoDto {
                                sku: u.sku,
                                nombre: u.nombre,
                                cantidad_millis: u.cantidad_millis,
                                vendido_cents: u.vendido_cents,
                                costo_cents: u.costo_cents,
                                utilidad_cents: u.utilidad_cents,
                            })
                            .collect(),
                    )
                })
                .map_err(|e| e.to_string()),
            Comando::CodigoPropio => Products::siguiente_ean_interno(self.db.conn())
                .map(Cuerpo::Texto)
                .map_err(|e| e.to_string()),
            Comando::CodigosDe { producto_id } => {
                Products::barcodes_de(self.db.conn(), producto_id)
                    .map(|lista| Cuerpo::Texto(lista.first().cloned().unwrap_or_default()))
                    .map_err(|e| e.to_string())
            }
            Comando::ImprimirEtiqueta {
                codigo,
                nombre,
                precio_cents,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let codigo = codigo.trim().to_string();
                if !pos_core::ean13_valido(&codigo) {
                    Err(
                        "El código tiene que ser un EAN-13 de 13 dígitos (el de control incluido)."
                            .into(),
                    )
                } else {
                    let conn = self.db.conn();
                    let ruta = Settings::get_or(conn, "printer_path", "");
                    if ruta.trim().is_empty() {
                        Err(
                            "Primero configura la impresora (Detectar o una prueba de ticket)."
                                .into(),
                        )
                    } else {
                        let baudios = Settings::as_i64(conn, "printer_baud", 9600).max(0) as u32;
                        let perfil_n = Settings::get_or(conn, "printer_profile", "generica-80");
                        let perfil = pos_hw::Perfil::por_nombre(&perfil_n)
                            .unwrap_or_default()
                            .con_baudios(baudios)
                            .con_columnas(Settings::as_i64(conn, "ticket_width", 48) as usize);
                        let precio = Money::from_cents(precio_cents).format_mxn();
                        let bytes = pos_hw::etiqueta_ean13(&perfil, &nombre, &precio, &codigo);
                        let transporte = pos_hw::Transporte::desde_texto(&ruta, baudios);
                        match transporte.imprimir(&perfil, &bytes) {
                            Ok(()) => {
                                auditoria::anotar(
                                    conn,
                                    Some(uid),
                                    "etiqueta",
                                    "impresora",
                                    None,
                                    &format!("{codigo} {nombre}"),
                                );
                                Ok(Cuerpo::Texto(format!(
                            "Etiqueta de {codigo} enviada a la impresora. Guarda el producto para que el lector lo encuentre."
                        )))
                            }
                            Err(e) => Err(format!("No se pudo imprimir la etiqueta: {e}")),
                        }
                    }
                }
            }
            Comando::CancelarFactura {
                id,
                motivo,
                uuid_sustitucion,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Facturar) {
                    return Respuesta::error(e);
                }
                let f = match fiscal::cancelar(self.db.conn(), id, &motivo, &uuid_sustitucion) {
                    Ok(f) => f,
                    Err(e) => return Respuesta::error(e),
                };
                auditoria::anotar(
                    self.db.conn(),
                    Some(uid),
                    "cancelar_cfdi",
                    "facturas",
                    Some(id),
                    &format!(
                        "folio {} motivo {} sustitución '{}'",
                        f.folio,
                        motivo.trim(),
                        uuid_sustitucion.trim()
                    ),
                );
                Ok(Cuerpo::Factura(Box::new(f)))
            }
            Comando::LeerConfig => {
                let conn = self.db.conn();
                Ok(Cuerpo::Configuracion(ConfigDto {
                    tienda: Settings::get_or(conn, "store_name", "Mi Tienda"),
                    direccion: Settings::get_or(conn, "store_address", ""),
                    pie: Settings::get_or(conn, "ticket_footer", "¡Gracias por su compra!"),
                    impresora: Settings::get_or(conn, "printer_path", ""),
                    ancho_ticket: Settings::as_i64(conn, "ticket_width", 48),
                    impresora_perfil: Settings::get_or(
                        conn,
                        "printer_profile",
                        &pos_hw::Perfil::default().nombre,
                    ),
                    impresora_baudios: Settings::as_i64(conn, "printer_baud", 9600),
                    abrir_cajon: Settings::get_or(conn, "printer_open_drawer", "1") == "1",
                    respaldo_automatico: Settings::get_or(conn, "backup_enabled", "1") == "1",
                    respaldo_carpeta: Settings::get_or(conn, "backup_folder", ""),
                    respaldo_cada_dias: Settings::as_i64(conn, "backup_every_days", 1),
                    respaldos_a_conservar: Settings::as_i64(conn, "backup_keep", 14),
                    respaldar_al_cortar: Settings::get_or(conn, "backup_on_close", "1") == "1",
                    puerto_lan: Settings::as_i64(conn, "lan_port", 8080),
                }))
            }
            Comando::GuardarConfig { config } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let conn = self.db.conn();
                auditoria::anotar(
                    conn,
                    Some(uid),
                    "configuracion",
                    "settings",
                    None,
                    &format!(
                        "tienda '{}', impresora '{}', respaldo {} en '{}'",
                        config.tienda,
                        config.impresora,
                        if config.respaldo_automatico {
                            "automático"
                        } else {
                            "manual"
                        },
                        if config.respaldo_carpeta.trim().is_empty() {
                            "la carpeta junto a la base"
                        } else {
                            config.respaldo_carpeta.trim()
                        }
                    ),
                );
                let r = Settings::set(conn, "store_name", &config.tienda)
                    .and_then(|_| Settings::set(conn, "store_address", &config.direccion))
                    .and_then(|_| Settings::set(conn, "ticket_footer", &config.pie))
                    .and_then(|_| Settings::set(conn, "printer_path", &config.impresora))
                    .and_then(|_| {
                        Settings::set(conn, "ticket_width", &config.ancho_ticket.to_string())
                    })
                    .and_then(|_| Settings::set(conn, "printer_profile", &config.impresora_perfil))
                    .and_then(|_| {
                        Settings::set(conn, "printer_baud", &config.impresora_baudios.to_string())
                    })
                    .and_then(|_| {
                        Settings::set(
                            conn,
                            "printer_open_drawer",
                            if config.abrir_cajon { "1" } else { "0" },
                        )
                    })
                    .and_then(|_| {
                        Settings::set(
                            conn,
                            "backup_enabled",
                            if config.respaldo_automatico { "1" } else { "0" },
                        )
                    })
                    .and_then(|_| Settings::set(conn, "backup_folder", &config.respaldo_carpeta))
                    .and_then(|_| {
                        Settings::set(
                            conn,
                            "backup_every_days",
                            &config.respaldo_cada_dias.to_string(),
                        )
                    })
                    .and_then(|_| {
                        Settings::set(
                            conn,
                            "backup_keep",
                            &config.respaldos_a_conservar.max(1).to_string(),
                        )
                    })
                    .and_then(|_| {
                        Settings::set(
                            conn,
                            "backup_on_close",
                            if config.respaldar_al_cortar { "1" } else { "0" },
                        )
                    })
                    .and_then(|_| {
                        let puerto = if config.puerto_lan <= 0 {
                            8080
                        } else {
                            config.puerto_lan.clamp(1, 65535)
                        };
                        Settings::set(conn, "lan_port", &puerto.to_string())
                    });
                r.map(|_| Cuerpo::Vacio).map_err(|e| e.to_string())
            }

            // --- impresora ---
            Comando::Impresoras => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                Ok(Cuerpo::Impresoras(
                    pos_hw::enumerar()
                        .into_iter()
                        .map(|i| ImpresoraDto {
                            ruta: i.ruta,
                            etiqueta: i.etiqueta,
                            fabricante: i.fabricante,
                            transporte: match &i.transporte {
                                pos_hw::Transporte::Archivo { .. } => "usb".into(),
                                pos_hw::Transporte::Serie { .. } => "serie".into(),
                                pos_hw::Transporte::Red { .. } => "red".into(),
                                pos_hw::Transporte::Spooler { .. } => "windows".into(),
                            },
                            baudios: i.perfil.baudios as i64,
                            perfil: i.perfil.nombre,
                            es_impresora: i.es_impresora,
                            nota: i.nota,
                        })
                        .collect(),
                ))
            }
            Comando::ProbarImpresion {
                ruta,
                baudios,
                perfil,
                abrir_cajon,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let conn = self.db.conn();
                let tienda = Settings::get_or(conn, "store_name", "Mi Tienda");
                let perfil = pos_hw::Perfil::por_nombre(&perfil)
                    .unwrap_or_default()
                    .con_baudios(baudios.max(0) as u32)
                    .con_columnas(Settings::as_i64(conn, "ticket_width", 48) as usize);
                let transporte = pos_hw::Transporte::desde_texto(&ruta, baudios.max(0) as u32);
                let bytes = pos_hw::ticket_de_prueba(&perfil, &tienda, &ruta, abrir_cajon);
                let total = bytes.len() as i64;
                let resultado = transporte.imprimir(&perfil, &bytes);
                if resultado.is_ok() {
                    // Si salió, ya es la impresora de la tienda. El instalador
                    // termina aquí: la siguiente venta imprime sola.
                    let _ = Settings::set(conn, "printer_path", &ruta);
                    let _ = Settings::set(conn, "printer_profile", &perfil.nombre);
                    let _ = Settings::set(conn, "printer_baud", &perfil.baudios.to_string());
                }
                auditoria::anotar(
                    conn,
                    Some(uid),
                    if resultado.is_ok() {
                        "impresion_prueba"
                    } else {
                        "impresion_prueba_fallida"
                    },
                    "impresora",
                    None,
                    &format!("{} ({total} bytes)", transporte.etiqueta()),
                );
                Ok(Cuerpo::Prueba(PruebaImpresionDto {
                    ok: resultado.is_ok(),
                    mensaje: match &resultado {
                        Ok(()) => format!(
                            "Ticket de prueba enviado a {} ({total} bytes). Mira acentos, la regla de números, el QR y el corte. ¿Salió completo? Quedó guardada.",
                            transporte.etiqueta()
                        ),
                        Err(e) => format!("No se pudo imprimir: {e}"),
                    },
                    bytes: total,
                    transporte: transporte.etiqueta(),
                    perfil: perfil.nombre.clone(),
                }))
            }
            Comando::Respaldos => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                Ok(Cuerpo::Respaldos(self.estado_respaldos()))
            }
            Comando::RespaldarAhora { carpeta } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let destino = match carpeta.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                    Some(c) => std::path::PathBuf::from(c),
                    None => self.carpeta_respaldo(),
                };
                let mut estado = self.estado_respaldos();
                match respaldo::crear(&self.db, &destino) {
                    Ok(info) => {
                        let borrados =
                            respaldo::rotar(&destino, self.cfg_i64("backup_keep", 14) as usize);
                        estado.mensaje = if borrados.is_empty() {
                            format!("Respaldo listo: {}", info.nombre)
                        } else {
                            format!(
                                "Respaldo listo: {}. Se borraron {} respaldos viejos.",
                                info.nombre,
                                borrados.len()
                            )
                        };
                        estado.carpeta = destino.display().to_string();
                        estado.ultimo = Some(RespaldoDto::from(info));
                        estado.total = respaldo::listar(&destino).len();
                        estado.bytes = respaldo::listar(&destino).iter().map(|r| r.bytes).sum();
                        estado.pendiente = false;
                    }
                    Err(e) => estado.mensaje = format!("No se pudo respaldar: {e}"),
                }
                Ok(Cuerpo::Respaldos(estado))
            }
            Comando::VerificarRespaldos => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let carpeta = self.carpeta_respaldo();
                let todos = respaldo::listar(&carpeta);
                if todos.is_empty() {
                    return Respuesta::error("Todavía no hay respaldos que revisar".to_string());
                }
                let mut buenos = 0;
                let mut danados: Vec<String> = Vec::new();
                for r in &todos {
                    match respaldo::verificar(&r.ruta) {
                        Ok(true) => buenos += 1,
                        _ => danados.push(r.nombre.clone()),
                    }
                }
                let texto = if danados.is_empty() {
                    format!(
                        "{buenos} respaldos revisados y todos sirven:\n{}",
                        carpeta.display()
                    )
                } else {
                    format!(
                        "{} respaldos buenos y {} DAÑADOS: {}\nHaz un respaldo nuevo hoy mismo.",
                        buenos,
                        danados.len(),
                        danados.join(", ")
                    )
                };
                Ok(Cuerpo::Texto(texto))
            }
            Comando::Auditoria {
                desde,
                hasta,
                usuario,
                accion,
                texto,
                limite,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::VerAuditoria) {
                    return Respuesta::error(e);
                }
                let filtro = auditoria::Filtro {
                    desde,
                    hasta,
                    usuario,
                    accion,
                    texto,
                    limite: if limite == 0 { 200 } else { limite },
                };
                match auditoria::buscar(self.db.conn(), &filtro) {
                    Ok(eventos) => Ok(Cuerpo::Auditoria(EventosAuditoriaDto {
                        mostrados: eventos.len(),
                        eventos: eventos
                            .into_iter()
                            .map(|e| EventoAuditoriaDto {
                                id: e.id,
                                usuario: e.usuario,
                                etiqueta: auditoria::etiqueta(&e.accion),
                                accion: e.accion,
                                detalle: e.detalle,
                                cuando: e.created_at,
                            })
                            .collect(),
                        acciones: auditoria::acciones(self.db.conn()).unwrap_or_default(),
                        total: auditoria::total(self.db.conn()).unwrap_or(0),
                    })),
                    Err(e) => Err(e.to_string()),
                }
            }
            // --- conteo físico -------------------------------------------
            Comando::IniciarConteo { categoria } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let filtro = categoria.clone().unwrap_or_default();
                match conteo::iniciar(self.db.conn_mut(), uid, &filtro).and_then(|id| {
                    let r = conteo::resumen(self.db.conn(), id)?;
                    Ok((id, r))
                }) {
                    Ok((id, _)) => {
                        auditoria::anotar(
                            self.db.conn(),
                            Some(uid),
                            "iniciar_conteo",
                            "stock_count",
                            Some(id),
                            &if filtro.is_empty() {
                                "conteo de todo el inventario".to_string()
                            } else {
                                format!("conteo de la categoría {filtro}")
                            },
                        );
                        self.conteo_dto(id)
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Comando::VerConteo { conteo_id } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                match match conteo_id {
                    Some(id) => conteo::por_id(self.db.conn(), id),
                    None => conteo::abierto(self.db.conn()),
                } {
                    Ok(Some(c)) => self.conteo_dto(c.id),
                    Ok(None) => Ok(Cuerpo::Conteo(ConteoDto::default())),
                    Err(e) => Err(e.to_string()),
                }
            }
            Comando::ContarProducto {
                codigo,
                cantidad_millis,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                let abierto = match conteo::abierto(self.db.conn()) {
                    Ok(Some(c)) => c,
                    Ok(None) => {
                        return Respuesta::error("No hay ningún conteo abierto".to_string())
                    }
                    Err(e) => return Respuesta::error(e.to_string()),
                };
                let codigo = codigo.trim();
                // Igual que en la venta: primero por código de barras, luego
                // por SKU. Contar se hace escaneando.
                let producto = match Products::by_barcode(self.db.conn(), codigo) {
                    Ok(Some(p)) => Some(p),
                    _ => Products::by_sku(self.db.conn(), codigo).ok().flatten(),
                };
                let producto = match producto {
                    Some(p) => p,
                    None => return Respuesta::error(format!("No existe el producto '{codigo}'")),
                };
                match conteo::contar(self.db.conn(), abierto.id, producto.id, cantidad_millis) {
                    Ok(()) => self.conteo_dto(abierto.id),
                    Err(e) => Err(e.to_string()),
                }
            }
            Comando::CerrarConteo {
                conteo_id,
                aplicar,
                supervisor: _,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                // Aplicar ajustes mueve el inventario: queda quién lo hizo.
                if aplicar {
                    let cajero = self.nombre_de(uid);
                    self.anotar_hecho(
                        uid,
                        "ajuste_inventario",
                        "stock_count",
                        &format!("{cajero} aplicó el conteo físico #{conteo_id}"),
                    );
                }
                match conteo::cerrar(self.db.conn_mut(), conteo_id, uid, aplicar) {
                    Ok(r) => {
                        auditoria::anotar(
                            self.db.conn(),
                            Some(uid),
                            "cerrar_conteo",
                            "stock_count",
                            Some(conteo_id),
                            &format!(
                                "{} productos contados, {} con diferencia, merma ${:.2}{}",
                                r.contados,
                                r.con_diferencia,
                                r.valor_merma_cents as f64 / 100.0,
                                if aplicar {
                                    " (ajustes aplicados)"
                                } else {
                                    " (sin aplicar)"
                                }
                            ),
                        );
                        Ok(Cuerpo::Texto(self.texto_conteo(&r, aplicar)))
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Comando::CancelarConteo { conteo_id } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                match conteo::cancelar(self.db.conn(), conteo_id) {
                    Ok(()) => Ok(Cuerpo::Vacio),
                    Err(e) => Err(e.to_string()),
                }
            }
            // --- actualizaciones firmadas -------------------------------
            Comando::EstadoActualizacion => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                Ok(Cuerpo::EstadoActualizacion(self.estado_actualizacion()))
            }
            Comando::RevisarActualizacion { ruta } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                Ok(Cuerpo::Paquete(self.revisar_actualizacion(&ruta)))
            }
            Comando::AplicarActualizacion { ruta } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                match self.aplicar_actualizacion(&ruta, Some(uid)) {
                    Ok(t) => Ok(Cuerpo::Texto(t)),
                    Err(e) => Err(e),
                }
            }
            Comando::RevertirActualizacion => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                match self.ejecutable() {
                    Ok(exe) => match pos_update::aplicar::revertir(&exe) {
                        Ok(()) => {
                            auditoria::anotar(
                                self.db.conn(),
                                Some(uid),
                                "revertir_actualizacion",
                                "programa",
                                None,
                                "volvió a la versión anterior",
                            );
                            Ok(Cuerpo::Texto(
                                "Se volvió a la versión anterior. Cierra y abre el programa."
                                    .to_string(),
                            ))
                        }
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(e),
                }
            }
            Comando::Proveedores { termino } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                pedidos::listar_proveedores(self.db.conn(), &termino)
                    .map(|v| {
                        Cuerpo::Proveedores(
                            v.into_iter()
                                .map(|p| ProveedorDto {
                                    id: p.id,
                                    name: p.name,
                                    phone: p.phone,
                                    nota: p.nota,
                                })
                                .collect(),
                        )
                    })
                    .map_err(|e| e.to_string())
            }
            Comando::GuardarProveedor { proveedor } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                match pedidos::guardar_proveedor(
                    self.db.conn(),
                    proveedor.id,
                    &proveedor.name,
                    &proveedor.phone,
                    &proveedor.nota,
                ) {
                    Ok(id) => {
                        auditoria::anotar(
                            self.db.conn(),
                            Some(uid),
                            "proveedor",
                            "proveedores",
                            Some(id),
                            &format!("proveedor {}", proveedor.name.trim()),
                        );
                        Ok(Cuerpo::Texto(id.to_string()))
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Comando::Pedidos { limite } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                pedidos::listar_pedidos(self.db.conn(), limite.max(1))
                    .map(|v| Cuerpo::Pedidos(v.into_iter().map(pedido_dto).collect()))
                    .map_err(|e| e.to_string())
            }
            Comando::VerPedido { id } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                pedidos::ver(self.db.conn(), id)
                    .map(|p| Cuerpo::Pedido(Box::new(pedido_dto(p))))
                    .map_err(|e| e.to_string())
            }
            Comando::NuevoPedido { proveedor_id, nota } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                match pedidos::nuevo(self.db.conn_mut(), proveedor_id, &nota, uid) {
                    Ok(p) => {
                        auditoria::anotar(
                            self.db.conn(),
                            Some(uid),
                            "pedido",
                            "pedidos",
                            Some(p.id),
                            &format!("pedido {} a {}", p.folio, p.proveedor),
                        );
                        Ok(Cuerpo::Pedido(Box::new(pedido_dto(p))))
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Comando::AgregarAPedido {
                pedido_id,
                codigo,
                qty_millis,
                costo_cents,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                pedidos::agregar(
                    self.db.conn_mut(),
                    pedido_id,
                    &codigo,
                    qty_millis,
                    costo_cents,
                )
                .map(|p| Cuerpo::Pedido(Box::new(pedido_dto(p))))
                .map_err(|e| e.to_string())
            }
            Comando::QuitarDePedido {
                pedido_id,
                product_id,
            } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                pedidos::quitar(self.db.conn_mut(), pedido_id, product_id)
                    .map(|p| Cuerpo::Pedido(Box::new(pedido_dto(p))))
                    .map_err(|e| e.to_string())
            }
            Comando::RecibirPedido { id, entregas } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                let lista: Vec<pedidos::Entrega> = entregas
                    .into_iter()
                    .map(|e| pedidos::Entrega {
                        product_id: e.product_id,
                        qty_millis: e.qty_millis,
                    })
                    .collect();
                match pedidos::recibir(self.db.conn_mut(), id, &lista, uid) {
                    Ok(p) => {
                        let piezas: i64 = lista.iter().map(|e| e.qty_millis).sum();
                        auditoria::anotar(
                            self.db.conn(),
                            Some(uid),
                            "recibir_pedido",
                            "pedidos",
                            Some(p.id),
                            &format!(
                                "recibió {} de {} ({} piezas)",
                                p.folio,
                                p.proveedor,
                                piezas / 1000
                            ),
                        );
                        Ok(Cuerpo::Pedido(Box::new(pedido_dto(p))))
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Comando::CancelarPedido { id } => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::AjusteInventario) {
                    return Respuesta::error(e);
                }
                match pedidos::cancelar(self.db.conn_mut(), id) {
                    Ok(p) => {
                        auditoria::anotar(
                            self.db.conn(),
                            Some(uid),
                            "cancelar_pedido",
                            "pedidos",
                            Some(p.id),
                            &format!("canceló {}", p.folio),
                        );
                        Ok(Cuerpo::Pedido(Box::new(pedido_dto(p))))
                    }
                    Err(e) => Err(e.to_string()),
                }
            }

            Comando::Unidades => {
                if let Err(e) = self.exigir_sesion(uid, Permiso::Configuracion) {
                    return Respuesta::error(e);
                }
                Ok(Cuerpo::Unidades(
                    respaldo::unidades()
                        .into_iter()
                        .map(|u| UnidadDto {
                            etiqueta: u.etiqueta,
                            ruta: u.ruta.display().to_string(),
                        })
                        .collect(),
                ))
            }
        };

        a_respuesta(resultado)
    }
}

impl From<pos_db::respaldo::RespaldoInfo> for RespaldoDto {
    fn from(r: pos_db::respaldo::RespaldoInfo) -> RespaldoDto {
        RespaldoDto {
            nombre: r.nombre,
            ruta: r.ruta.display().to_string(),
            bytes: r.bytes,
            creado: r.creado,
        }
    }
}

fn pedido_dto(p: pedidos::Pedido) -> PedidoDto {
    let total = p.total_costo_cents();
    PedidoDto {
        id: p.id,
        folio: p.folio,
        proveedor_id: p.proveedor_id,
        proveedor: p.proveedor,
        estado: p.estado,
        nota: p.nota,
        created_at: p.created_at,
        total_costo_cents: total,
        lineas: p
            .lineas
            .into_iter()
            .map(|l| LineaPedidoDto {
                product_id: l.product_id,
                sku: l.sku,
                nombre: l.nombre,
                qty_pedida_millis: l.qty_pedida_millis,
                qty_recibida_millis: l.qty_recibida_millis,
                costo_cents: l.costo_cents,
            })
            .collect(),
    }
}

fn a_respuesta(r: Result<Cuerpo, String>) -> Respuesta {
    match r {
        Ok(c) => Respuesta::Ok(c),
        Err(e) => Respuesta::Error(e),
    }
}

/// Fecha de hoy en horario de la Ciudad de México (`YYYY-MM-DD`).
pub fn hoy_cdmx() -> String {
    let segs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        - 6 * 3600;
    let dias = segs.div_euclid(86_400);
    let (anio, mes, dia) = civil_desde_dias(dias);
    format!("{anio:04}-{mes:02}-{dia:02}")
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

/// Si no mandan rango, es el día de hoy.
fn normalizar_rango(desde: &str, hasta: &str) -> (String, String) {
    let hoy = hoy_cdmx();
    let desde = if desde.trim().is_empty() {
        hoy.clone()
    } else {
        desde.trim().to_string()
    };
    let hasta = if hasta.trim().is_empty() {
        hoy
    } else {
        hasta.trim().to_string()
    };
    (desde, hasta)
}

/// Convierte una venta guardada en un ticket imprimible.
fn ticket_dto(v: &pos_db::sales::SaleRecord, db: &pos_db::Database) -> TicketDto {
    let conn = db.conn();
    let cajero = v
        .user_id
        .and_then(|id| pos_db::users::Users::by_id(conn, id).ok().flatten())
        .map(|u| u.display_name)
        .unwrap_or_default();
    let cliente = v
        .customer_id
        .and_then(|id| pos_db::clientes::Clientes::por_id(conn, id).ok().flatten())
        .map(|c| c.name)
        .unwrap_or_default();

    TicketDto {
        folio: v.folio.clone(),
        fecha: v.created_at.clone(),
        cajero,
        cliente,
        estado: v.status.clone(),
        lineas: v
            .items
            .iter()
            .map(|i| TicketLineaDto {
                sku: i.sku.clone(),
                descripcion: i.description.clone(),
                qty_millis: i.qty_millis,
                precio_cents: i.unit_price_cents,
                descuento_cents: i.discount_cents,
                importe_cents: i.unit_price_cents.saturating_mul(i.qty_millis) / 1_000
                    - i.discount_cents,
            })
            .collect(),
        subtotal_cents: v.subtotal_cents,
        descuento_cents: v.discount_cents,
        impuesto_cents: v.tax_cents,
        total_cents: v.total_cents,
        pagos: v
            .payments
            .iter()
            .map(|p| PagoDto {
                metodo: p.method.clone(),
                monto_cents: p.amount_cents,
                recibido_cents: p.received_cents,
            })
            .collect(),
        cambio_cents: v
            .payments
            .iter()
            .filter(|p| p.method == "efectivo")
            .map(|p| (p.received_cents - p.amount_cents).max(0))
            .sum(),
    }
}

/// Las ventas de un periodo como CSV, para el contador.
fn ventas_a_csv(filas: &[FilaVenta]) -> String {
    let mut salida =
        String::from("folio,fecha,cajero,subtotal,descuento,impuesto,total,pagos,cambio,estado\n");
    for f in filas {
        let cajero = f.cajero.replace('"', "''");
        let pagos = f.pagos.replace('"', "''");
        salida.push_str(&format!(
            "{},{},\"{}\",{:.2},{:.2},{:.2},{:.2},\"{}\",{:.2},{}\n",
            f.folio,
            f.fecha,
            cajero,
            f.subtotal_cents as f64 / 100.0,
            f.descuento_cents as f64 / 100.0,
            f.impuesto_cents as f64 / 100.0,
            f.total_cents as f64 / 100.0,
            pagos,
            f.cambio_cents as f64 / 100.0,
            f.estado,
        ));
    }
    salida
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PosApi;

    /// Crea una base en memoria con admin, un supervisor, un cajero y dos
    /// productos. Es el escenario base de todas las pruebas.
    fn escenario() -> LocalApi {
        let api = LocalApi::new(Database::open_in_memory().unwrap());
        poblar(api)
    }

    /// Igual que `escenario` pero sobre un archivo real: hace falta para
    /// probar respaldos, que son el tema de esta sección.
    fn escenario_en_archivo(dir: &std::path::Path) -> LocalApi {
        let api = LocalApi::new(Database::open(dir.join("tienda.db")).unwrap());
        poblar(api)
    }

    fn poblar(mut api: LocalApi) -> LocalApi {
        // La base está vacía: creamos los usuarios directo (la app lo hace
        // con `asegurar_admin`).
        Users::crear(
            api.db.conn_mut(),
            "admin",
            "Administrador",
            "1234",
            pos_core::Role::Admin,
        )
        .unwrap();
        Users::crear(
            api.db.conn_mut(),
            "jefe",
            "El Jefe",
            "2222",
            pos_core::Role::Supervisor,
        )
        .unwrap();
        Users::crear(
            api.db.conn_mut(),
            "ana",
            "Ana",
            "4321",
            pos_core::Role::Cajero,
        )
        .unwrap();
        for (sku, nombre, precio) in [
            ("7501", "Coca-Cola 600 ml", 1800),
            ("7502", "Pan blanco", 1850),
        ] {
            Products::insert(
                api.db.conn_mut(),
                &NewProduct {
                    sku: sku.into(),
                    name: nombre.into(),
                    price_cents: precio,
                    cost_cents: precio / 2,
                    tax_bp: 1600,
                    unit: "pza".into(),
                    stock_millis: 100_000,
                    min_stock_millis: 0,
                    track_inventory: true,
                    barcodes: vec![],
                },
            )
            .unwrap();
        }
        api
    }

    fn entrar(api: &mut LocalApi, usuario: &str, pin: &str) -> String {
        match api.login(usuario, pin) {
            Ok(s) => s.token,
            Err(e) => panic!("no pudo entrar {usuario}: {e}"),
        }
    }

    fn abrir_caja(api: &mut LocalApi, token: &str, fondo: i64) {
        let r = api.ejecutar(Peticion {
            token: Some(token.into()),
            comando: Comando::AbrirCaja { fondo_cents: fondo },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "abrir caja: {r:?}");
    }

    // -------------------------------------------- facturación (CFDI 4.0) ----

    /// El día civil (hora de México) de la última venta guardada. Se usa el
    /// mismo criterio que la consulta de la factura global, así la prueba no
    /// se cae a las siete de la noche por culpa del UTC.
    fn dia_de_la_ultima_venta(api: &LocalApi) -> String {
        api.db()
            .conn()
            .query_row(
                "SELECT date(created_at, '-6 hours') FROM sales ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
    }

    fn config_fiscal_valida() -> ConfigFiscalDto {
        ConfigFiscalDto {
            rfc: "XAXX010101000".into(),
            nombre: "Abarrotes Lupita".into(),
            regimen_fiscal: "626".into(),
            codigo_postal: "91260".into(),
            forma_pago: "01".into(),
            periodicidad: "01".into(),
            carpeta_salida: String::new(),
            carpeta_entrada: String::new(),
        }
    }

    /// Deja el escenario listo para facturar: caja abierta, dos ventas y los
    /// datos fiscales puestos. Devuelve el día que hay que facturar.
    fn listo_para_facturar(api: &mut LocalApi) -> String {
        let t = entrar(api, "admin", "1234");
        abrir_caja(api, &t, 0);
        api.cobrar(&t, venta("7501", 2, 1_800)).unwrap();
        api.cobrar(&t, venta("7502", 1, 1_850)).unwrap();
        let r = api.ejecutar(Peticion {
            token: Some(t),
            comando: Comando::GuardarConfigFiscal {
                config: config_fiscal_valida(),
            },
        });
        assert!(
            matches!(r, Respuesta::Ok(_)),
            "guardar datos fiscales: {r:?}"
        );
        dia_de_la_ultima_venta(api)
    }

    fn factura_global(api: &mut LocalApi, token: &str, dia: &str) -> Respuesta {
        api.ejecutar(Peticion {
            token: Some(token.into()),
            comando: Comando::FacturaGlobal {
                fecha: dia.into(),
                periodicidad: "01".into(),
            },
        })
    }

    #[test]
    fn un_cajero_no_puede_emitir_una_factura() {
        let mut api = escenario();
        let dia = listo_para_facturar(&mut api);
        let t_ana = entrar(&mut api, "ana", "4321");
        match factura_global(&mut api, &t_ana, &dia) {
            Respuesta::Error(e) => assert!(
                e.contains("facturas") || e.contains("permiso") || e.contains("Facturar"),
                "el error debe decir qué permiso falta: {e}"
            ),
            otro => panic!("un cajero no debe poder facturar: {otro:?}"),
        }
    }

    #[test]
    fn un_pac_por_http_se_rechaza_y_queda_anotado() {
        let mut api = escenario();
        let t = entrar(&mut api, "admin", "1234");
        let mut cfg = config_fiscal_valida();
        cfg.carpeta_salida = "https://api.sw.com.mx/v4/cfdi33/issue".into();
        cfg.carpeta_entrada = "https://api.sw.com.mx/v4/cfdi33/stamp".into();
        let r = api.ejecutar(Peticion {
            token: Some(t.clone()),
            comando: Comando::GuardarConfigFiscal { config: cfg },
        });
        match r {
            Respuesta::Error(e) => {
                assert!(e.contains("bloqueado"), "{e}");
                assert!(e.contains("carpetas"), "{e}");
            }
            otro => panic!("no se configura un PAC por HTTP: {otro:?}"),
        }
        let t_jefe = entrar(&mut api, "jefe", "2222");
        let a: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "pac_http")
            .collect();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].usuario, "Administrador");
        assert!(
            a[0].detalle.contains("https://api.sw.com.mx"),
            "{}",
            a[0].detalle
        );
        // Y no quedó guardado: las carpetas siguen vacías.
        match api.ejecutar(Peticion {
            token: Some(t),
            comando: Comando::ConfigFiscal,
        }) {
            Respuesta::Ok(Cuerpo::ConfigFiscal(c)) => {
                assert!(c.carpeta_salida.is_empty());
                assert!(c.carpeta_entrada.is_empty());
            }
            otro => panic!("{otro:?}"),
        }
    }

    #[test]
    fn un_cajero_no_puede_cambiar_el_rfc_del_negocio() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        let r = api.ejecutar(Peticion {
            token: Some(t_ana),
            comando: Comando::GuardarConfigFiscal {
                config: config_fiscal_valida(),
            },
        });
        match r {
            Respuesta::Error(e) => assert!(!e.is_empty()),
            otro => panic!("un cajero no debe cambiar los datos fiscales: {otro:?}"),
        }
    }

    #[test]
    fn el_encargado_puede_facturar_pero_no_cambiar_el_rfc() {
        let mut api = escenario();
        let dia = listo_para_facturar(&mut api);
        let t_jefe = entrar(&mut api, "jefe", "2222");

        // Facturar el día: sí (para eso es encargado).
        match factura_global(&mut api, &t_jefe, &dia) {
            Respuesta::Ok(Cuerpo::Factura(f)) => {
                assert_eq!(f.total_cents, 1800 * 2 + 1850, "no cuadró con lo vendido");
                assert_eq!(f.estado, "pendiente");
            }
            otro => panic!("el encargado debe poder facturar: {otro:?}"),
        }

        // Cambiar la identidad fiscal del negocio: no.
        let r = api.ejecutar(Peticion {
            token: Some(t_jefe),
            comando: Comando::GuardarConfigFiscal {
                config: config_fiscal_valida(),
            },
        });
        assert!(
            matches!(r, Respuesta::Error(_)),
            "el encargado no cambia el RFC"
        );
    }

    #[test]
    fn un_cajero_no_puede_cancelar_un_cfdi() {
        let mut api = escenario();
        let dia = listo_para_facturar(&mut api);
        let t_jefe = entrar(&mut api, "jefe", "2222");
        let id = match factura_global(&mut api, &t_jefe, &dia) {
            Respuesta::Ok(Cuerpo::Factura(f)) => f.id,
            otro => panic!("{otro:?}"),
        };
        let t_ana = entrar(&mut api, "ana", "4321");
        let r = api.ejecutar(Peticion {
            token: Some(t_ana),
            comando: Comando::CancelarFactura {
                id,
                motivo: "02".into(),
                uuid_sustitucion: String::new(),
            },
        });
        assert!(
            matches!(r, Respuesta::Error(_)),
            "el cajero no cancela CFDI: {r:?}"
        );
    }

    #[test]
    fn un_codigo_propio_es_ean13_interno() {
        let mut api = escenario();
        let t = entrar(&mut api, "admin", "1234");
        let r = api.ejecutar(Peticion {
            token: Some(t.clone()),
            comando: Comando::CodigoPropio,
        });
        match r {
            Respuesta::Ok(Cuerpo::Texto(c)) => {
                assert!(pos_core::ean13_valido(&c), "{c}");
                assert!(c.starts_with("20"));
            }
            otro => panic!("{otro:?}"),
        }
        let r2 = api.ejecutar(Peticion {
            token: Some(t),
            comando: Comando::UtilidadHoy { limite: 5 },
        });
        assert!(matches!(r2, Respuesta::Ok(Cuerpo::Utilidad(_))), "{r2:?}");
    }

    /// Un cajero que puede mover el inventario puede tapar un faltante: sube
    /// las cajas que faltan y el almacén cuadra aunque el dinero no esté.
    #[test]
    fn un_cajero_no_mueve_el_inventario() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        let id = api
            .buscar(&t, "7501", 1)
            .expect("el producto de prueba")
            .remove(0)
            .id;
        let antes = api.buscar(&t, "7501", 1).unwrap()[0].stock_millis;

        let r = api.ejecutar(Peticion {
            token: Some(t.clone()),
            comando: Comando::AjusteInventario {
                producto_id: id,
                delta_millis: 24_000,
                motivo: "entrada del proveedor".into(),
            },
        });
        assert!(
            matches!(r, Respuesta::Error(_)),
            "sin permiso, el almacén no se mueve: {r:?}"
        );

        // El encargado sí, y el stock cambia.
        let t_jefe = entrar(&mut api, "admin", "1234");
        let r = api.ejecutar(Peticion {
            token: Some(t_jefe),
            comando: Comando::AjusteInventario {
                producto_id: id,
                delta_millis: 24_000,
                motivo: "entrada del proveedor".into(),
            },
        });
        assert!(r.es_ok(), "el encargado sí surte: {r:?}");
        assert_eq!(
            api.buscar(&t, "7501", 1).unwrap()[0].stock_millis,
            antes + 24_000,
            "de veras entraron 24"
        );
    }

    /// La lista de usuarios dice quién manda en la tienda. Sin sesión no se
    /// reparte, ni aunque la pida alguien conectado por la red.
    #[test]
    fn la_lista_de_usuarios_no_se_reparte_a_cualquiera() {
        let mut api = escenario();

        // La del login sí es pública, pero no trae roles.
        match api.ejecutar(Peticion {
            token: None,
            comando: Comando::UsuariosLogin,
        }) {
            Respuesta::Ok(Cuerpo::Usuarios(u)) => {
                assert!(!u.is_empty(), "la pantalla de login necesita los nombres");
                assert!(
                    u.iter().all(|x| x.role.is_empty()),
                    "el login no necesita saber el rol de nadie"
                );
            }
            otro => panic!("la lista del login es pública: {otro:?}"),
        }

        // La completa, sin sesión: no.
        let r = api.ejecutar(Peticion {
            token: None,
            comando: Comando::Usuarios,
        });
        assert!(
            matches!(r, Respuesta::Error(_)),
            "sin iniciar sesión no se entregan los roles"
        );

        // Un cajero tampoco la ve.
        let t = entrar(&mut api, "ana", "4321");
        let r = api.ejecutar(Peticion {
            token: Some(t),
            comando: Comando::Usuarios,
        });
        assert!(
            matches!(r, Respuesta::Error(_)),
            "un cajero no ve la lista de usuarios"
        );

        // Quien administra, sí.
        let t_jefe = entrar(&mut api, "admin", "1234");
        match api.ejecutar(Peticion {
            token: Some(t_jefe),
            comando: Comando::Usuarios,
        }) {
            Respuesta::Ok(Cuerpo::Usuarios(u)) => assert!(!u.is_empty()),
            otro => panic!("quien administra sí ve la lista: {otro:?}"),
        }
    }

    #[test]
    fn la_factura_global_queda_en_la_bitacora_con_su_importe() {
        let mut api = escenario();
        let dia = listo_para_facturar(&mut api);
        let t = entrar(&mut api, "admin", "1234");
        factura_global(&mut api, &t, &dia);

        let eventos = bitacora(&mut api, &t);
        let anotada = eventos
            .iter()
            .find(|e| e.accion == "factura_global")
            .unwrap_or_else(|| panic!("no se anotó la factura: {eventos:?}"));
        assert!(anotada.detalle.contains("$54.50"), "{}", anotada.detalle);
        assert!(anotada.detalle.contains("2 ventas"), "{}", anotada.detalle);
    }

    #[test]
    fn reintentar_un_timbre_tambien_pide_permiso() {
        let mut api = escenario();
        let dia = listo_para_facturar(&mut api);
        let t_admin = entrar(&mut api, "admin", "1234");
        let id = match factura_global(&mut api, &t_admin, &dia) {
            Respuesta::Ok(Cuerpo::Factura(f)) => f.id,
            otro => panic!("no se generó: {otro:?}"),
        };
        let t_ana = entrar(&mut api, "ana", "4321");
        let r = api.ejecutar(Peticion {
            token: Some(t_ana),
            comando: Comando::ReintentarFactura { id },
        });
        assert!(
            matches!(r, Respuesta::Error(_)),
            "un cajero no reintenta timbres"
        );
    }

    #[test]
    fn sin_sesion_no_se_factura_nada() {
        let mut api = escenario();
        let r = api.ejecutar(Peticion {
            token: None,
            comando: Comando::FacturaGlobal {
                fecha: "2026-10-11".into(),
                periodicidad: "01".into(),
            },
        });
        match r {
            Respuesta::Error(e) => assert!(e.contains("sesión") || e.contains("Sesión"), "{e}"),
            otro => panic!("sin sesión no debe facturar: {otro:?}"),
        }
    }

    // ------------------------------------------------------- auditoría ----

    fn bitacora(api: &mut LocalApi, token: &str) -> Vec<EventoAuditoriaDto> {
        match api.ejecutar(Peticion {
            token: Some(token.into()),
            comando: Comando::Auditoria {
                desde: String::new(),
                hasta: String::new(),
                usuario: String::new(),
                accion: String::new(),
                texto: String::new(),
                limite: 300,
            },
        }) {
            Respuesta::Ok(Cuerpo::Auditoria(a)) => a.eventos,
            otro => panic!("bitácora: {otro:?}"),
        }
    }

    #[test]
    fn un_cambio_de_precio_queda_anotado_con_quien_lo_cobro() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        let t_jefe = entrar(&mut api, "jefe", "2222");
        abrir_caja(&mut api, &t_ana, 0);

        // Ana cobra con el precio cambiado a mano: no pide encargado,
        // pero queda escrito que lo hizo ella.
        api.cobrar(&t_ana, venta("7501", 1, 1_500)).unwrap();

        let precios: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "precio")
            .collect();
        assert_eq!(precios.len(), 1, "tiene que quedar una sola");
        let a = &precios[0];
        assert_eq!(a.usuario, "Ana");
        assert!(
            a.detalle.contains("7501") && a.detalle.contains("15.00"),
            "el detalle tiene que decir qué producto y a qué precio: {}",
            a.detalle
        );
        assert!(
            bitacora(&mut api, &t_jefe)
                .iter()
                .all(|e| e.accion != "autorizar"),
            "cambiar el precio ya no es una autorización"
        );
    }

    #[test]
    fn precio_en_cero_significa_el_del_catalogo_no_un_regalo() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t_ana, 0);

        // 0 = "usa el precio del catálogo". No debe pedir autorización ni
        // cobrar a $0.00: lo primero traba la caja, lo segundo regala el
        // producto. Con este bug una app móvil no podía vender.
        let mut dto = venta("7501", 2, 0);
        dto.pagos = vec![PagoDto {
            metodo: "efectivo".into(),
            monto_cents: 3_600,
            recibido_cents: 3_600,
        }];
        let r = api.cobrar(&t_ana, dto);
        assert!(r.is_ok(), "una venta normal no pide supervisor: {r:?}");
        assert_eq!(r.unwrap().total_cents, 3_600, "se cobra el precio de lista");

        // Y la bitácora lo dice con el precio real.
        let t_jefe = entrar(&mut api, "jefe", "2222");
        let ventas: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "venta")
            .collect();
        assert_eq!(ventas.len(), 1);
        assert!(
            ventas[0].detalle.contains("7501 x2") && ventas[0].detalle.contains("36.00"),
            "el detalle permite buscar por producto: {}",
            ventas[0].detalle
        );
    }

    #[test]
    fn un_cajero_puede_cambiar_el_precio_sin_encargado() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t_ana, 0);
        // $15.00 en vez de los $18.00 de lista: Ana lo cobra y listo.
        let r = api.cobrar(&t_ana, venta("7501", 1, 1_500));
        assert!(r.is_ok(), "el precio a mano no pide PIN: {r:?}");
        assert_eq!(r.unwrap().total_cents, 1_500);
    }

    #[test]
    fn un_descuento_queda_anotado_con_el_importe() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        let t_jefe = entrar(&mut api, "jefe", "2222");
        abrir_caja(&mut api, &t_ana, 0);

        let mut dto = venta("7501", 1, 1_800);
        dto.lineas[0].descuento_cents = 300;
        dto.pagos[0].monto_cents = 1_500;
        dto.pagos[0].recibido_cents = 1_500;
        api.cobrar(&t_ana, dto).unwrap();

        let a: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "descuento")
            .collect();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].usuario, "Ana");
        assert!(
            a[0].detalle.contains("3.00") && a[0].detalle.contains("7501"),
            "debe decir cuánto y en qué producto: {}",
            a[0].detalle
        );
    }

    #[test]
    fn un_corte_que_no_cuadra_deja_anotada_la_diferencia_y_quien_la_firmo() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        let t_jefe = entrar(&mut api, "jefe", "2222");
        abrir_caja(&mut api, &t_ana, 10_000); // fondo de $100

        // Cierra contando $0: no cuadra, Ana lo cierra y queda escrito.
        let r = api.ejecutar(Peticion {
            token: Some(t_ana.clone()),
            comando: Comando::CerrarCaja {
                piezas: vec![(100i64, 0)],
                supervisor: None,
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "corte: {r:?}");

        let a: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "diferencia")
            .collect();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].usuario, "Ana");
        assert!(
            a[0].detalle.contains("no cuadra") && a[0].detalle.contains("diferencia"),
            "tiene que quedar la diferencia: {}",
            a[0].detalle
        );
    }

    #[test]
    fn un_descuento_sin_pin_se_cobra_y_queda_anotado() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t_ana, 0);
        let mut dto = venta("7501", 1, 1_800);
        dto.lineas[0].descuento_cents = 300;
        dto.pagos[0].monto_cents = 1_500;
        dto.pagos[0].recibido_cents = 1_500;
        let ok = api.cobrar(&t_ana, dto).unwrap();
        assert_eq!(ok.total_cents, 1_500);
        let t_jefe = entrar(&mut api, "jefe", "2222");
        let a: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "descuento")
            .collect();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].usuario, "Ana");
    }

    #[test]
    fn el_pin_equivocado_y_el_bloqueo_quedan_anotados() {
        let mut api = escenario();
        let t_admin = entrar(&mut api, "admin", "1234");
        // Tres intentos malos seguidos.
        for _ in 0..3 {
            assert!(api.login("ana", "0000").is_err());
        }
        let eventos = bitacora(&mut api, &t_admin);
        let fallidos = eventos
            .iter()
            .filter(|e| e.accion == "login_fallido")
            .count();
        assert_eq!(fallidos, 3, "cada intento fallido se anota: {eventos:?}");
        assert!(
            eventos.iter().any(|e| e.accion == "login"),
            "el login bueno del admin también"
        );
    }

    #[test]
    fn la_bitacora_se_filtra_por_accion_y_por_texto() {
        let mut api = escenario();
        let t_admin = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &t_admin, 0);
        api.cobrar(&t_admin, venta("7501", 1, 1_800)).unwrap();

        let solo_ventas = api.ejecutar(Peticion {
            token: Some(t_admin.clone()),
            comando: Comando::Auditoria {
                desde: String::new(),
                hasta: String::new(),
                usuario: String::new(),
                accion: "venta".into(),
                texto: String::new(),
                limite: 50,
            },
        });
        match solo_ventas {
            Respuesta::Ok(Cuerpo::Auditoria(a)) => {
                assert_eq!(a.mostrados, 1);
                assert!(a.total >= 3, "la bitácora completa tiene más: {}", a.total);
                assert!(a.acciones.contains(&"venta".to_string()));
                assert!(
                    a.eventos[0].detalle.contains("000001"),
                    "{}",
                    a.eventos[0].detalle
                );
            }
            otro => panic!("filtro: {otro:?}"),
        }
    }

    #[test]
    fn la_auditoria_es_territorio_del_encargado() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        // Un cajero no puede andar viendo la bitácora.
        match api.ejecutar(Peticion {
            token: Some(t_ana),
            comando: Comando::Auditoria {
                desde: String::new(),
                hasta: String::new(),
                usuario: String::new(),
                accion: String::new(),
                texto: String::new(),
                limite: 50,
            },
        }) {
            Respuesta::Error(e) => assert!(e.contains("permiso"), "{e}"),
            otro => panic!("un cajero no debe ver la auditoría: {otro:?}"),
        }
    }

    #[test]
    fn el_respaldo_queda_en_la_bitacora() {
        let dir = tempfile::tempdir().unwrap();
        let mut api = escenario_en_archivo(dir.path());
        carpeta_temporal(&api, dir.path());
        let token = entrar(&mut api, "admin", "1234");
        api.respaldar(true).unwrap();

        let eventos = bitacora(&mut api, &token);
        let r: Vec<_> = eventos.iter().filter(|e| e.accion == "respaldo").collect();
        assert_eq!(r.len(), 1, "cada respaldo se anota: {eventos:?}");
        assert!(r[0].detalle.contains(".db"), "{}", r[0].detalle);
    }

    // ------------------------------------------------------- respaldos ----

    /// Deja la configuración de respaldo apuntando a una carpeta temporal.
    fn carpeta_temporal(api: &LocalApi, dir: &std::path::Path) -> String {
        let carpeta = dir.join("respaldos");
        Settings::set(api.db.conn(), "backup_folder", carpeta.to_str().unwrap()).unwrap();
        carpeta.to_str().unwrap().to_string()
    }

    #[test]
    fn la_configuracion_del_respaldo_se_guarda_y_se_lee() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarConfig {
                config: ConfigDto {
                    tienda: "Abarrotes Lupita".into(),
                    direccion: "Av. Central 12".into(),
                    pie: "Gracias".into(),
                    impresora: String::new(),
                    ancho_ticket: 48,
                    impresora_perfil: pos_hw::Perfil::default().nombre,
                    impresora_baudios: 9600,
                    abrir_cajon: true,
                    respaldo_automatico: true,
                    respaldo_carpeta: "/media/usb/respaldos".into(),
                    respaldo_cada_dias: 3,
                    respaldos_a_conservar: 7,
                    respaldar_al_cortar: false,
                    puerto_lan: 8080,
                },
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "guardar config: {r:?}");

        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::LeerConfig,
        }) {
            Respuesta::Ok(Cuerpo::Configuracion(c)) => {
                assert_eq!(c.respaldo_carpeta, "/media/usb/respaldos");
                assert_eq!(c.respaldo_cada_dias, 3);
                assert_eq!(c.respaldos_a_conservar, 7);
                assert!(c.respaldo_automatico);
                assert!(!c.respaldar_al_cortar);
            }
            otro => panic!("leer config: {otro:?}"),
        }
    }

    #[test]
    fn respaldar_ahora_deja_un_archivo_verificado_y_actualiza_el_estado() {
        let dir = tempfile::tempdir().unwrap();
        let mut api = escenario_en_archivo(dir.path());
        let carpeta = carpeta_temporal(&api, dir.path());
        let token = entrar(&mut api, "admin", "1234");

        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::RespaldarAhora { carpeta: None },
        }) {
            Respuesta::Ok(Cuerpo::Respaldos(e)) => {
                assert_eq!(e.total, 1);
                assert!(e.bytes > 0, "el respaldo no puede pesar cero");
                assert!(
                    e.mensaje.contains("Respaldo listo"),
                    "mensaje: {}",
                    e.mensaje
                );
                assert!(e.ultimo.is_some());
                assert!(!e.pendiente);
                assert_eq!(e.carpeta, carpeta);
                // Y el archivo existe de verdad en el disco.
                let ultimo = e.ultimo.unwrap();
                assert!(std::path::Path::new(&ultimo.ruta).exists());
            }
            otro => panic!("respaldar: {otro:?}"),
        }
    }

    #[test]
    fn la_rotacion_borra_los_viejos_y_deja_los_ultimos() {
        let dir = tempfile::tempdir().unwrap();
        let mut api = escenario_en_archivo(dir.path());
        let carpeta = carpeta_temporal(&api, dir.path());
        Settings::set(api.db.conn(), "backup_keep", "2").unwrap();
        let token = entrar(&mut api, "admin", "1234");

        for _ in 0..4 {
            assert!(matches!(
                api.ejecutar(Peticion {
                    token: Some(token.clone()),
                    comando: Comando::RespaldarAhora { carpeta: None },
                }),
                Respuesta::Ok(_)
            ));
            // Los respaldos del mismo segundo chocarían de nombre: se espera
            // un poquito para que la marca de tiempo cambie.
            std::thread::sleep(std::time::Duration::from_millis(1_100));
        }

        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::Respaldos,
        }) {
            Respuesta::Ok(Cuerpo::Respaldos(e)) => assert_eq!(
                e.total, 2,
                "sólo se conservan los dos últimos, había {}",
                e.total
            ),
            otro => panic!("estado: {otro:?}"),
        }
        assert_eq!(
            std::fs::read_dir(&carpeta)
                .unwrap()
                .filter(|f| f
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("respaldo-"))
                .count(),
            2
        );
    }

    #[test]
    fn verificar_respaldos_detecta_uno_daniado() {
        let dir = tempfile::tempdir().unwrap();
        let mut api = escenario_en_archivo(dir.path());
        carpeta_temporal(&api, dir.path());
        let token = entrar(&mut api, "admin", "1234");
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::RespaldarAhora { carpeta: None },
        });

        // Primero: todo está bien.
        match api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::VerificarRespaldos,
        }) {
            Respuesta::Ok(Cuerpo::Texto(t)) => assert!(t.contains("todos sirven"), "{t}"),
            otro => panic!("verificar: {otro:?}"),
        }

        // Ahora se le escribe basura encima al único respaldo.
        let carpeta = api.carpeta_respaldo();
        let archivo = respaldo::listar(&carpeta).pop().unwrap();
        let mut contenido = std::fs::read(&archivo.ruta).unwrap();
        for byte in contenido.iter_mut().take(200).skip(100) {
            *byte = 0;
        }
        std::fs::write(&archivo.ruta, contenido).unwrap();

        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::VerificarRespaldos,
        }) {
            Respuesta::Ok(Cuerpo::Texto(t)) => {
                assert!(t.contains("DAÑADOS"), "debe avisar del daño: {t}");
                assert!(t.contains("Haz un respaldo nuevo"));
            }
            otro => panic!("verificar dañado: {otro:?}"),
        }
    }

    #[test]
    fn el_tick_no_hace_nada_si_esta_apagado_y_lo_hace_si_no_hay_ninguno() {
        let dir = tempfile::tempdir().unwrap();
        let api = escenario_en_archivo(dir.path());
        carpeta_temporal(&api, dir.path());

        // Apagado: ni toca el disco.
        Settings::set(api.db.conn(), "backup_enabled", "0").unwrap();
        assert!(api.tick_respaldo().is_none());
        assert_eq!(respaldo::listar(&api.carpeta_respaldo()).len(), 0);

        // Encendido y sin respaldos: hace uno.
        Settings::set(api.db.conn(), "backup_enabled", "1").unwrap();
        let mensaje = api.tick_respaldo().expect("debió respaldar");
        assert!(mensaje.contains("Respaldo listo"), "{mensaje}");
        assert_eq!(respaldo::listar(&api.carpeta_respaldo()).len(), 1);

        // Recién hecho: no vuelve a respaldar.
        assert!(api.tick_respaldo().is_none());
        assert_eq!(respaldo::listar(&api.carpeta_respaldo()).len(), 1);
    }

    #[test]
    fn al_cortar_la_caja_queda_un_respaldo_hecho() {
        let dir = tempfile::tempdir().unwrap();
        let mut api = escenario_en_archivo(dir.path());
        carpeta_temporal(&api, dir.path());
        Settings::set(api.db.conn(), "backup_on_close", "1").unwrap();
        let token = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &token, 0);
        assert!(matches!(
            api.ejecutar(Peticion {
                token: Some(token.clone()),
                comando: Comando::Cobrar {
                    venta: venta("7501", 1, 1800)
                },
            }),
            Respuesta::Ok(_)
        ));

        // Corte con arqueo exacto: $10 + $5 + $2 + $1 = los $18.00 vendidos.
        let piezas = vec![(1000i64, 1i64), (500, 1), (200, 1), (100, 1)];
        let r = api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::CerrarCaja {
                piezas,
                supervisor: None,
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "corte: {r:?}");
        assert_eq!(
            respaldo::listar(&api.carpeta_respaldo()).len(),
            1,
            "cerrar la caja deja un respaldo del día"
        );
    }

    #[test]
    fn si_el_respaldo_al_cortar_falla_el_corte_no_se_pierde() {
        let dir = tempfile::tempdir().unwrap();
        let mut api = escenario_en_archivo(dir.path());
        // Carpeta imposible: el respaldo va a fallar.
        Settings::set(
            api.db.conn(),
            "backup_folder",
            "/esta/ruta/no/existe/ni/existira/respaldos",
        )
        .unwrap();
        Settings::set(api.db.conn(), "backup_on_close", "1").unwrap();
        let token = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &token, 0);
        let piezas = vec![(0i64, 0i64)];
        let r = api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::CerrarCaja {
                piezas,
                supervisor: None,
            },
        });
        assert!(
            matches!(r, Respuesta::Ok(_)),
            "el corte se debe guardar aunque el respaldo falle: {r:?}"
        );
    }

    #[test]
    fn pedir_los_respaldos_sin_sesion_es_un_error_claro() {
        let dir = tempfile::tempdir().unwrap();
        let mut api = escenario_en_archivo(dir.path());
        let r = api.ejecutar(Peticion {
            token: None,
            comando: Comando::Respaldos,
        });
        match r {
            Respuesta::Error(e) => assert!(e.contains("inicia sesión"), "{e}"),
            otro => panic!("sin sesión debe fallar: {otro:?}"),
        }
    }

    #[test]
    fn las_unidades_se_piden_por_el_api() {
        let dir = tempfile::tempdir().unwrap();
        let mut api = escenario_en_archivo(dir.path());
        let token = entrar(&mut api, "admin", "1234");
        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::Unidades,
        }) {
            Respuesta::Ok(Cuerpo::Unidades(u)) => {
                // Puede que esta máquina no tenga nada montado, pero la
                // respuesta llega y trae rutas utilizables.
                for unidad in u {
                    assert!(!unidad.etiqueta.is_empty());
                    assert!(!unidad.ruta.is_empty());
                }
            }
            otro => panic!("unidades: {otro:?}"),
        }
    }

    fn venta(sku: &str, qty: i64, precio: i64) -> VentaDto {
        VentaDto {
            lineas: vec![LineaDto {
                sku: sku.into(),
                descripcion: sku.into(),
                qty_millis: qty * 1000,
                precio_cents: precio,
                descuento_cents: 0,
            }],
            pagos: vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: precio * qty,
                recibido_cents: precio * qty,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn login_y_token_funcionan() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        // Con el token se puede operar.
        let r = api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::EstadoCaja,
        });
        assert!(matches!(r, Respuesta::Ok(Cuerpo::Caja(_))));
    }

    #[test]
    fn sin_token_no_se_puede_operar() {
        let mut api = escenario();
        let r = api.ejecutar(Peticion {
            token: None,
            comando: Comando::EstadoCaja,
        });
        assert!(matches!(r, Respuesta::Error(_)));

        let r = api.ejecutar(Peticion {
            token: Some("token-falso".into()),
            comando: Comando::EstadoCaja,
        });
        assert!(matches!(r, Respuesta::Error(_)));
    }

    #[test]
    fn pin_equivocado_da_error_y_bloquea() {
        let mut api = escenario();
        for _ in 0..5 {
            assert!(api.login("ana", "0000").is_err());
        }
        // Bloqueado: ya ni con el PIN bueno.
        assert!(api.login("ana", "4321").is_err());
    }

    #[test]
    fn no_se_puede_vender_sin_caja_abierta() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        let r = api.cobrar(&token, venta("7501", 2, 1800));
        assert!(r.is_err());
    }

    #[test]
    fn el_ciclo_completo_del_dia_por_api() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &token, 50_000);

        let vendida = api.cobrar(&token, venta("7501", 3, 1800)).unwrap();
        assert_eq!(vendida.total_cents, 5400);
        assert_eq!(vendida.folio, "000001");

        let caja = api.estado_caja(&token).unwrap();
        assert!(caja.abierta);
        assert_eq!(caja.ventas_efectivo_cents, 5400);
        assert_eq!(caja.esperado_cents, 55_400); // fondo 500 + venta 54

        // Cierra con arqueo exacto: 1x$500 + 55 monedas... mejor billetes.
        let piezas: Vec<(i64, i64)> = DENOMINACIONES.iter().map(|d| (*d, 0)).collect();
        let mut piezas = piezas;
        asignar(&mut piezas, 20_000, 2); // 400
        asignar(&mut piezas, 10_000, 1); // 100
        asignar(&mut piezas, 5_000, 1); // 50
        asignar(&mut piezas, 200, 2); // 2 monedas de $2 = $4
        let total: i64 = piezas.iter().map(|(v, c)| v * c).sum();
        assert_eq!(total, 55_400);

        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::CerrarCaja {
                piezas,
                supervisor: None,
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "cierre: {r:?}");
        let caja = api.estado_caja(&token).unwrap();
        assert!(!caja.abierta);
    }

    fn asignar(piezas: &mut [(i64, i64)], denominacion: i64, cantidad: i64) {
        if let Some(p) = piezas.iter_mut().find(|(d, _)| *d == denominacion) {
            p.1 = cantidad;
        }
    }

    const DENOMINACIONES: [i64; 14] = [
        5, 10, 20, 50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000, 50_000, 100_000,
    ];

    #[test]
    fn un_arqueo_que_no_cuadra_lo_cierra_el_cajero_y_queda_anotado() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &token, 50_000);
        api.cobrar(&token, venta("7501", 3, 1800)).unwrap();

        let mut piezas: Vec<(i64, i64)> = DENOMINACIONES.iter().map(|d| (*d, 0)).collect();
        asignar(&mut piezas, 20_000, 2); // $400, esperado $554
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::CerrarCaja {
                piezas,
                supervisor: None,
            },
        });
        assert!(
            matches!(r, Respuesta::Ok(_)),
            "sin PIN también cierra: {r:?}"
        );
        let t_jefe = entrar(&mut api, "jefe", "2222");
        let a: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "diferencia")
            .collect();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].usuario, "Ana");
    }

    #[test]
    fn un_cajero_puede_descontar_sin_pin() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &token, 0);

        let mut v = venta("7501", 2, 1800);
        v.descuento_global_cents = 500;
        let ok = api.cobrar(&token, v).unwrap();
        assert_eq!(ok.total_cents, 3100); // 36 - 5
        assert_eq!(ok.descuento_cents, 500);
    }

    #[test]
    fn un_cajero_puede_retirar_y_queda_anotado() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &token, 50_000);
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::MovimientoCaja {
                tipo: "retiro".into(),
                monto_cents: 5_000,
                motivo: "pago al panadero".into(),
                supervisor: None,
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "retiro sin PIN: {r:?}");
        let t_jefe = entrar(&mut api, "jefe", "2222");
        let a: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "retiro")
            .collect();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].usuario, "Ana");
        assert!(a[0].detalle.contains("panadero"), "{}", a[0].detalle);
    }

    #[test]
    fn cambiar_el_precio_a_mano_no_exige_supervisor() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &token, 0);

        let v = venta("7501", 1, 1500); // el catálogo dice 1800
        let ok = api.cobrar(&token, v).unwrap();
        assert_eq!(ok.total_cents, 1500);
    }

    #[test]
    fn el_fiado_requiere_cliente_y_limite_no_pin() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &token, 0);

        let cliente_id = match api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarCliente {
                cliente: NuevoClienteDto {
                    name: "Doña Mary".into(),
                    phone: "5512345678".into(),
                    credit_limit_cents: 30_000,
                    ..Default::default()
                },
            },
        }) {
            Respuesta::Ok(Cuerpo::Texto(id)) => id.parse::<i64>().unwrap(),
            otro => panic!("alta de cliente: {otro:?}"),
        };

        // Fiado sin cliente: no pasa.
        let mut v = venta("7501", 5, 1800); // $90
        v.pagos = vec![PagoDto {
            metodo: "credito".into(),
            monto_cents: 9000,
            recibido_cents: 0,
        }];
        assert!(api.cobrar(&token, v.clone()).is_err());

        // Con cliente, sin PIN: pasa y queda debiendo $90.
        v.cliente_id = Some(cliente_id);
        api.cobrar(&token, v).unwrap();
        let t_jefe = entrar(&mut api, "jefe", "2222");
        let a: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "fiado")
            .collect();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].usuario, "Ana");
        assert!(a[0].detalle.contains("Mary"), "{}", a[0].detalle);

        let lista = api.clientes(&token, "Mary").unwrap();
        assert_eq!(lista[0].saldo_cents, 9000);
        assert_eq!(lista[0].disponible_cents, 21_000);

        // Abona $40 y queda debiendo $50.
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::Abonar {
                cliente_id,
                monto_cents: 4000,
                metodo: "efectivo".into(),
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)));
        let lista = api.clientes(&token, "Mary").unwrap();
        assert_eq!(lista[0].saldo_cents, 5000);

        // El fiado no mete efectivo a la caja.
        let caja = api.estado_caja(&token).unwrap();
        assert_eq!(caja.ventas_efectivo_cents, 0);
    }

    #[test]
    fn no_se_fia_mas_del_limite() {
        let mut api = escenario();
        let token = entrar(&mut api, "jefe", "2222");
        abrir_caja(&mut api, &token, 0);

        let cliente_id = 1;
        Clientes::crear(
            api.db.conn_mut(),
            &NuevoCliente {
                name: "Pepe".into(),
                rfc: String::new(),
                phone: String::new(),
                email: String::new(),
                credit_limit_cents: 1_000, // $10
                regimen_fiscal: String::new(),
                codigo_postal: String::new(),
                uso_cfdi: String::new(),
            },
        )
        .unwrap();

        let mut v = venta("7501", 5, 1800); // $90 > límite
        v.pagos = vec![PagoDto {
            metodo: "credito".into(),
            monto_cents: 9000,
            recibido_cents: 0,
        }];
        v.cliente_id = Some(cliente_id);
        assert!(api.cobrar(&token, v).is_err());
    }

    // =====================================================================
    // Fase 2: catálogo, importación CSV e inventario
    // =====================================================================

    fn csv_de_prueba(renglones: usize) -> String {
        let mut csv = String::from("sku,descripcion,precio,costo,iva,existencia\n");
        for i in 0..renglones {
            csv.push_str(&format!("P{i},Producto {i},10.50,7.00,16,25\n"));
        }
        csv
    }

    #[test]
    fn importar_un_csv_crea_el_catalogo() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");

        let csv = "\
sku,descripcion,precio,costo,iva,existencia,codigo de barras
7503,Coca-Cola 600 ml,$18.50,12.00,16,120,7501055300000
7504,Pan de caja,18.50,13,16,30,7501000123457
";
        let r = api.importar_csv(&token, csv, false).unwrap();
        assert!(r.sin_errores(), "{:?}", r.errores);
        assert_eq!(r.creados, 2);
        assert_eq!(r.actualizados, 0);

        let productos = api.buscar(&token, "caja", 10).unwrap();
        assert_eq!(productos.len(), 1);
        assert_eq!(productos[0].price_cents, 1850);
        assert_eq!(productos[0].stock_millis, 30_000);

        // El código de barras quedó ligado al producto: ya se puede escanear.
        let escaneado = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::ProductoPorCodigo {
                codigo: "7501055300000".into(),
            },
        });
        match escaneado {
            Respuesta::Ok(Cuerpo::Productos(p)) => {
                assert_eq!(p.len(), 1);
                assert_eq!(p[0].sku, "7503");
            }
            otro => panic!("se esperaba el producto, llegó {otro:?}"),
        }
    }

    #[test]
    fn importar_dos_veces_falla_y_dice_el_renglon() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let csv = csv_de_prueba(2);
        api.importar_csv(&token, &csv, false).unwrap();

        let r = api.importar_csv(&token, &csv, false).unwrap();
        assert!(!r.sin_errores());
        assert_eq!(r.creados, 0);
        assert!(
            r.errores[0].motivo.contains("P0") && r.errores[0].motivo.contains("ya existe"),
            "{}",
            r.errores[0].motivo
        );

        // Con la bandera de actualizar, la segunda pasa.
        let r = api.importar_csv(&token, &csv, true).unwrap();
        assert!(r.sin_errores(), "{:?}", r.errores);
        assert_eq!(r.actualizados, 2);
        assert_eq!(r.creados, 0);
    }

    #[test]
    fn un_renglon_malo_no_deja_el_catalogo_a_medias() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");

        let csv = "\
sku,nombre,precio,existencia
A1,Bueno,10.00,5
A2,Precio roto,caro,5
A3,Tercero,20.00,5
";
        let r = api.importar_csv(&token, csv, false).unwrap();
        assert_eq!(r.creados, 2, "los dos buenos entran");
        assert_eq!(r.errores.len(), 1);
        assert_eq!(r.errores[0].renglon, 3);
        assert_eq!(api.buscar(&token, "Bueno", 10).unwrap().len(), 1);
        assert_eq!(api.buscar(&token, "Tercero", 10).unwrap().len(), 1);
    }

    #[test]
    fn diez_mil_renglones_entran_en_menos_de_treinta_segundos() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let csv = csv_de_prueba(10_000);

        let inicio = std::time::Instant::now();
        let r = api.importar_csv(&token, &csv, false).unwrap();
        let transcurrido = inicio.elapsed();

        assert!(r.sin_errores(), "{:?}", r.errores);
        assert_eq!(r.creados, 10_000);
        assert!(
            transcurrido.as_secs() < 30,
            "la importación tardó {transcurrido:?}"
        );
        println!(">> 10,000 renglones importados en {transcurrido:?}");
    }

    #[test]
    fn exportar_y_volver_a_importar_reconstruye_el_catalogo() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        api.importar_csv(&token, &csv_de_prueba(5), false).unwrap();

        let csv = api.exportar_csv(&token).unwrap();
        assert!(csv.starts_with("sku,nombre,precio,costo,iva,existencia,minimo"));

        // Otra tienda reconstruye el catálogo desde ese respaldo. Los dos
        // productos que ya tenía se sobreescriben sin error.
        let mut otra = escenario();
        let token2 = entrar(&mut otra, "admin", "1234");
        let r = otra.importar_csv(&token2, &csv, true).unwrap();
        assert!(r.sin_errores(), "{:?}", r.errores);
        assert_eq!(r.creados, 5);
        assert_eq!(r.actualizados, 2);
        assert_eq!(otra.buscar(&token2, "Producto 3", 10).unwrap().len(), 1);
        let p = otra.buscar(&token2, "Producto 3", 10).unwrap();
        assert_eq!(p[0].price_cents, 1_050);
        assert_eq!(p[0].stock_millis, 25_000);
    }

    #[test]
    fn un_cajero_no_puede_importar_ni_dar_de_baja() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        assert!(api.importar_csv(&token, &csv_de_prueba(2), false).is_err());
        assert!(api
            .ejecutar(Peticion {
                token: Some(token),
                comando: Comando::DesactivarProducto { id: 1 },
            })
            .es_error());
    }

    #[test]
    fn el_cajero_puede_dar_de_alta_un_producto() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarProducto {
                producto: NuevoProductoDto {
                    id: None,
                    sku: "9002".into(),
                    name: "Jabón".into(),
                    price_cents: 1_750,
                    cost_cents: 1_100,
                    tax_bp: 1_600,
                    unit: "pza".into(),
                    stock_millis: 5_000,
                    min_stock_millis: 0,
                    track_inventory: true,
                    barcodes: vec![],
                    categoria: None,
                    clave_sat: None,
                    clave_unidad: None,
                    unidad_sat: None,
                },
            },
        });
        assert!(r.es_ok(), "el cajero da de alta: {r:?}");
        assert_eq!(api.buscar(&token, "Jabón", 1).unwrap()[0].sku, "9002");
    }

    #[test]
    fn el_admin_da_de_alta_a_una_persona_y_el_cajero_no() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        assert!(
            api.ejecutar(Peticion {
                token: Some(t_ana),
                comando: Comando::CrearUsuario {
                    username: "luis".into(),
                    display_name: "Luis".into(),
                    pin: "5555".into(),
                    role: "cajero".into(),
                },
            })
            .es_error(),
            "el cajero no administra gente"
        );

        let t_admin = entrar(&mut api, "admin", "1234");
        assert!(api
            .ejecutar(Peticion {
                token: Some(t_admin.clone()),
                comando: Comando::CrearUsuario {
                    username: "luis".into(),
                    display_name: "Luis".into(),
                    pin: "5555".into(),
                    role: "cajero".into(),
                },
            })
            .es_ok());
        assert!(entrar(&mut api, "luis", "5555").len() > 8);

        let lista = match api.ejecutar(Peticion {
            token: Some(t_admin.clone()),
            comando: Comando::Usuarios,
        }) {
            Respuesta::Ok(Cuerpo::Usuarios(u)) => u,
            otro => panic!("{otro:?}"),
        };
        assert!(lista.iter().any(|u| u.username == "luis"));

        let luis = lista.iter().find(|u| u.username == "luis").unwrap().id;
        assert!(api
            .ejecutar(Peticion {
                token: Some(t_admin.clone()),
                comando: Comando::DesactivarUsuario { id: luis },
            })
            .es_ok());
        assert!(
            api.ejecutar(Peticion {
                token: Some(t_admin),
                comando: Comando::Login {
                    username: "luis".into(),
                    pin: "5555".into(),
                },
            })
            .es_error(),
            "dado de baja no entra"
        );
    }

    #[test]
    fn no_se_puede_dejar_la_tienda_sin_admin() {
        let mut api = escenario();
        let t = entrar(&mut api, "admin", "1234");
        let admin_id = match api.ejecutar(Peticion {
            token: Some(t.clone()),
            comando: Comando::Usuarios,
        }) {
            Respuesta::Ok(Cuerpo::Usuarios(u)) => {
                u.into_iter().find(|x| x.username == "admin").unwrap().id
            }
            otro => panic!("{otro:?}"),
        };
        assert!(api
            .ejecutar(Peticion {
                token: Some(t.clone()),
                comando: Comando::DesactivarUsuario { id: admin_id },
            })
            .es_error());
        assert!(api
            .ejecutar(Peticion {
                token: Some(t),
                comando: Comando::ActualizarUsuario {
                    id: admin_id,
                    display_name: "Administrador".into(),
                    role: "cajero".into(),
                },
            })
            .es_error());
    }

    #[test]
    fn editar_un_producto_cambia_el_precio_pero_no_el_stock() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let original = api.buscar(&token, "coca", 1).unwrap()[0].clone();

        let dto = NuevoProductoDto {
            id: Some(original.id),
            sku: original.sku.clone(),
            name: "Coca-Cola 600 ml nueva formula".into(),
            price_cents: 2_000,
            cost_cents: 1_300,
            tax_bp: 1_600,
            unit: "pza".into(),
            stock_millis: 999_000, // debe ignorarse: el stock no se edita
            min_stock_millis: 10_000,
            track_inventory: true,
            barcodes: vec!["7501055399999".into()],
            categoria: None,
            clave_sat: None,
            clave_unidad: None,
            unidad_sat: None,
        };
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarProducto { producto: dto },
        });
        assert!(r.es_ok(), "{r:?}");

        let actualizado = api.buscar(&token, "nueva formula", 1).unwrap();
        assert_eq!(actualizado.len(), 1);
        assert_eq!(actualizado[0].price_cents, 2_000);
        assert_eq!(actualizado[0].min_stock_millis, 10_000);
        assert_eq!(actualizado[0].stock_millis, 100_000, "el stock no se toca");
    }

    #[test]
    fn la_entrada_de_mercancia_sube_el_stock_y_deja_rastro() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let id = api.buscar(&token, "coca", 1).unwrap()[0].id;

        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::AjusteInventario {
                producto_id: id,
                delta_millis: 24_000,
                motivo: "compra al proveedor".into(),
            },
        });
        assert!(r.es_ok(), "{r:?}");
        assert_eq!(
            api.buscar(&token, "coca", 1).unwrap()[0].stock_millis,
            124_000
        );

        // La merma lo baja.
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::AjusteInventario {
                producto_id: id,
                delta_millis: -4_000,
                motivo: "merma por caducidad".into(),
            },
        });
        assert_eq!(
            api.buscar(&token, "coca", 1).unwrap()[0].stock_millis,
            120_000
        );
    }

    #[test]
    fn un_ajuste_sin_motivo_no_pasa() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let id = api.buscar(&token, "coca", 1).unwrap()[0].id;
        let r = api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::AjusteInventario {
                producto_id: id,
                delta_millis: 1_000,
                motivo: "  ".into(),
            },
        });
        assert!(r.es_error());
    }

    #[test]
    fn dar_de_baja_un_producto_lo_saca_del_mostrador() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let id = api.buscar(&token, "coca", 1).unwrap()[0].id;
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::DesactivarProducto { id },
        });
        assert!(api.buscar(&token, "coca", 10).unwrap().is_empty());
    }

    // =====================================================================
    // Actualizaciones firmadas
    // =====================================================================

    /// Un "programa" de mentiritas: un script que se puede ejecutar de verdad.
    fn programa(contenido: &str) -> Vec<u8> {
        format!("#!/bin/sh\n{contenido}\n").into_bytes()
    }

    /// Deja un ejecutable falso en su propio directorio y lo devuelve.
    fn instalar_en(dir: &std::path::Path, contenido: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let exe = dir.join("pos");
        std::fs::write(&exe, programa(contenido)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        exe
    }

    /// Firma un paquete con la llave de desarrollo, que es la que trae el
    /// binario compilado: es el camino real de una actualización.
    fn firmar(dir: &std::path::Path, version: &str, contenido: &str) -> std::path::PathBuf {
        let privada = pos_update::llave_privada_de_desarrollo().expect("llave de desarrollo");
        let bytes = pos_update::paquete::armar(
            version,
            "0.0.1",
            &format!("notas de la {version}"),
            &programa(contenido),
            &privada,
        )
        .unwrap();
        let ruta = dir.join(format!("actualizacion-{version}.posup"));
        std::fs::write(&ruta, bytes).unwrap();
        ruta
    }

    fn directorio(nombre: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "pos-api-{nombre}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn paquete(api: &mut LocalApi, token: &str, ruta: &str) -> PaqueteDto {
        match api.ejecutar(Peticion {
            token: Some(token.into()),
            comando: Comando::RevisarActualizacion {
                ruta: ruta.to_string(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Paquete(p)) => p,
            otro => panic!("revisar actualización: {otro:?}"),
        }
    }

    #[test]
    fn revisar_un_paquete_dice_que_trae_y_que_es_nuestro() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let dir = directorio("revisar");
        let ruta = firmar(&dir, "99.0.0", "echo soy la nueva");

        let p = paquete(&mut api, &token, ruta.to_str().unwrap());
        assert!(p.valido, "debía ser válido: {}", p.motivo);
        assert_eq!(p.version, "99.0.0");
        assert_eq!(p.notas, "notas de la 99.0.0");
        assert!(p.tamanio > 0);
        assert_eq!(p.huella.len(), 12);
        assert!(p.motivo.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_paquete_que_no_es_nuestro_se_rechaza_y_lo_dice() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let dir = directorio("ajeno");
        // Firmado con otra llave: el ejecutable da igual, la firma no es de aquí.
        let bytes =
            pos_update::paquete::armar("99.0.0", "0.0.1", "", &programa("echo hola"), &[42u8; 32])
                .unwrap();
        let ruta = dir.join("ajeno.posup");
        std::fs::write(&ruta, bytes).unwrap();

        let p = paquete(&mut api, &token, ruta.to_str().unwrap());
        assert!(!p.valido);
        assert!(p.motivo.contains("no salió de nosotros"), "{}", p.motivo);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_paquete_de_mentiras_no_se_instala() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let dir = directorio("mentira");
        let ruta = dir.join("no-es.posup");
        std::fs::write(&ruta, b"esto no es un paquete").unwrap();

        let p = paquete(&mut api, &token, ruta.to_str().unwrap());
        assert!(!p.valido);
        assert!(p.motivo.contains("no es un paquete"), "{}", p.motivo);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Ejecuta un "falso exe" sh: en Windows no es Win32 válido (error 193).
    // La ruta con clausura (rollback sin ejecutar) sí se prueba en Windows.
    #[cfg(unix)]
    #[test]
    fn aplicar_una_actualizacion_deja_la_nueva_y_guarda_la_vieja() {
        let dir = directorio("aplicar");
        let exe = instalar_en(&dir, "echo soy la 0.4.0");
        let original = std::fs::read(&exe).unwrap();
        let mut api =
            LocalApi::new(pos_db::Database::open_in_memory().unwrap()).con_ejecutable(exe.clone());
        Users::crear(
            api.db.conn_mut(),
            "admin",
            "Administrador",
            "1234",
            pos_core::Role::Admin,
        )
        .unwrap();
        let token = entrar(&mut api, "admin", "1234");

        let paquete = firmar(&dir, "99.0.0", "echo soy la 99.0.0");
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::AplicarActualizacion {
                ruta: paquete.to_str().unwrap().to_string(),
            },
        });
        let Respuesta::Ok(Cuerpo::Texto(t)) = r else {
            panic!("aplicar: falló");
        };
        assert!(t.contains("99.0.0"), "{t}");
        assert!(!t.contains("Se dejó la versión"), "{t}");

        // El ejecutable cambió...
        let instalado = std::fs::read_to_string(&exe).unwrap();
        assert!(instalado.contains("99.0.0"));
        assert_ne!(std::fs::read(&exe).unwrap(), original);
        // ...y el anterior quedó guardado.
        let respaldo = std::fs::read_to_string(dir.join("pos.anterior")).unwrap();
        assert!(respaldo.contains("0.4.0"));
        // Quedó registro de la última actualización, sin pendientes.
        assert!(!dir.join("pos.pendiente").exists());
        assert!(dir.join("pos.ultima").exists());

        // Y el estado lo dice.
        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::EstadoActualizacion,
        }) {
            Respuesta::Ok(Cuerpo::EstadoActualizacion(e)) => {
                assert!(e.hay_respaldo);
                assert!(!e.hay_pendiente);
                assert_eq!(e.version_respaldo.as_deref(), Some(pos_update::VERSION));
            }
            otro => panic!("estado: {otro:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn si_el_nuevo_programa_no_arranca_se_deshecha_solo() {
        let dir = directorio("deshecha");
        let exe = instalar_en(&dir, "echo funciono");
        let original = std::fs::read(&exe).unwrap();
        let mut api =
            LocalApi::new(pos_db::Database::open_in_memory().unwrap()).con_ejecutable(exe.clone());
        Users::crear(
            api.db.conn_mut(),
            "admin",
            "Administrador",
            "1234",
            pos_core::Role::Admin,
        )
        .unwrap();
        let token = entrar(&mut api, "admin", "1234");

        // Un "programa" que falla al arrancar: sale con error.
        let paquete = firmar(&dir, "99.0.0", "exit 7");
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::AplicarActualizacion {
                ruta: paquete.to_str().unwrap().to_string(),
            },
        });
        let Respuesta::Ok(Cuerpo::Texto(t)) = r else {
            panic!("aplicar: falló");
        };
        assert!(t.contains("Se dejó la versión"), "{t}");
        // El ejecutable es exactamente el que sí funcionaba.
        assert_eq!(std::fs::read(&exe).unwrap(), original);

        // Y quedó anotado en la bitácora, con el motivo.
        let eventos = bitacora(&mut api, &token);
        let anotado = eventos
            .iter()
            .any(|e| e.accion == "actualizacion_deshecha" && e.detalle.contains("99.0.0"));
        assert!(anotado, "se anota el intento: {:?}", eventos.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn revertir_regresa_al_programa_anterior() {
        let dir = directorio("revertir");
        let exe = instalar_en(&dir, "echo la 0.4.0");
        let original = std::fs::read(&exe).unwrap();
        let mut api =
            LocalApi::new(pos_db::Database::open_in_memory().unwrap()).con_ejecutable(exe.clone());
        Users::crear(
            api.db.conn_mut(),
            "admin",
            "Administrador",
            "1234",
            pos_core::Role::Admin,
        )
        .unwrap();
        let token = entrar(&mut api, "admin", "1234");

        let paquete = firmar(&dir, "99.0.0", "echo la 99");
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::AplicarActualizacion {
                ruta: paquete.to_str().unwrap().to_string(),
            },
        });
        assert!(std::fs::read_to_string(&exe).unwrap().contains("99"));

        let r = api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::RevertirActualizacion,
        });
        assert!(matches!(r, Respuesta::Ok(_)), "{r:?}");
        assert_eq!(std::fs::read(&exe).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_cajero_no_puede_actualizar_el_programa() {
        let mut api = escenario();
        let ana = entrar(&mut api, "ana", "4321");
        let r = api.ejecutar(Peticion {
            token: Some(ana),
            comando: Comando::EstadoActualizacion,
        });
        assert!(r.es_error(), "un cajero no toca el programa: {r:?}");
    }

    // =====================================================================
    // Conteo físico asistido
    // =====================================================================

    /// El producto tal como está en la base, para comparar después del conteo.
    fn stock_de(api: &LocalApi, sku: &str) -> i64 {
        Products::by_sku(api.db.conn(), sku)
            .unwrap()
            .unwrap()
            .stock_millis
    }

    fn conteo(api: &mut LocalApi, token: &str, cmd: Comando) -> ConteoDto {
        match api.ejecutar(Peticion {
            token: Some(token.into()),
            comando: cmd,
        }) {
            Respuesta::Ok(Cuerpo::Conteo(c)) => c,
            otro => panic!("conteo: {otro:?}"),
        }
    }

    fn error_de(api: &mut LocalApi, token: &str, cmd: Comando) -> String {
        match api.ejecutar(Peticion {
            token: Some(token.into()),
            comando: cmd,
        }) {
            Respuesta::Error(e) => e,
            otro => panic!("se esperaba error y llegó: {otro:?}"),
        }
    }

    #[test]
    fn un_conteo_toma_la_foto_y_cuenta_escaneando() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");

        let c = conteo(&mut api, &token, Comando::IniciarConteo { categoria: None });
        // La foto: los dos productos, nadie ha contado nada.
        assert_eq!(c.productos, 2);
        assert_eq!(c.contados, 0);
        assert_eq!(c.renglones.len(), 2);
        assert!(c.renglones.iter().all(|r| r.contado_millis.is_none()));
        assert_eq!(c.estado, "abierto");

        // Se cuenta por código: escaneando el producto, no escribiendo el id.
        let c = conteo(
            &mut api,
            &token,
            Comando::ContarProducto {
                codigo: "7501".into(),
                cantidad_millis: 96_000,
            },
        );
        assert_eq!(c.contados, 1);
        // 96 contadas contra 100 del sistema: faltan 4.
        let coca = c.renglones.iter().find(|r| r.sku == "7501").unwrap();
        assert_eq!(coca.esperado_millis, 100_000);
        assert_eq!(coca.contado_millis, Some(96_000));
        assert_eq!(coca.diferencia_millis, Some(-4_000));
    }

    #[test]
    fn contar_dos_veces_deja_la_ultima() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        conteo(&mut api, &token, Comando::IniciarConteo { categoria: None });
        let cmd = |millis| Comando::ContarProducto {
            codigo: "7501".into(),
            cantidad_millis: millis,
        };
        conteo(&mut api, &token, cmd(96_000));
        // Se volvió a contar bien: 98. No se suman, se reemplaza.
        let c = conteo(&mut api, &token, cmd(98_000));
        assert_eq!(c.contados, 1);
        let coca = c.renglones.iter().find(|r| r.sku == "7501").unwrap();
        assert_eq!(coca.contado_millis, Some(98_000));
    }

    #[test]
    fn contar_un_codigo_que_no_existe_avisa_y_no_cuenta() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        conteo(&mut api, &token, Comando::IniciarConteo { categoria: None });
        let e = error_de(
            &mut api,
            &token,
            Comando::ContarProducto {
                codigo: "750999999".into(),
                cantidad_millis: 1_000,
            },
        );
        assert!(e.contains("No existe el producto"), "{e}");
    }

    #[test]
    fn contar_sin_conteo_abierto_es_un_error_claro() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let e = error_de(
            &mut api,
            &token,
            Comando::ContarProducto {
                codigo: "7501".into(),
                cantidad_millis: 1_000,
            },
        );
        assert!(e.contains("No hay ningún conteo abierto"), "{e}");
    }

    #[test]
    fn no_se_abren_dos_conteos_a_la_vez() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        conteo(&mut api, &token, Comando::IniciarConteo { categoria: None });
        let e = error_de(&mut api, &token, Comando::IniciarConteo { categoria: None });
        assert!(e.contains("Ya hay un conteo abierto"), "{e}");
    }

    #[test]
    fn cerrar_sin_aplicar_no_mueve_el_inventario() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let id = conteo(&mut api, &token, Comando::IniciarConteo { categoria: None }).id;
        conteo(
            &mut api,
            &token,
            Comando::ContarProducto {
                codigo: "7501".into(),
                cantidad_millis: 96_000,
            },
        );
        // Cerrar de a mentiritas: se ve el reporte, pero nada se mueve.
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::CerrarConteo {
                conteo_id: id,
                aplicar: false,
                supervisor: None,
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "{r:?}");
        assert_eq!(stock_de(&api, "7501"), 100_000);
        // Y ya no está abierto: se puede empezar otro.
        let otro = conteo(&mut api, &token, Comando::IniciarConteo { categoria: None });
        assert_eq!(otro.contados, 0);
    }

    #[test]
    fn aplicar_el_conteo_cuadra_el_inventario_y_queda_anotado() {
        let mut api = escenario();
        let admin = entrar(&mut api, "admin", "1234");

        let id = conteo(&mut api, &admin, Comando::IniciarConteo { categoria: None }).id;
        conteo(
            &mut api,
            &admin,
            Comando::ContarProducto {
                codigo: "7501".into(),
                cantidad_millis: 96_000,
            },
        );

        // Sin segundo PIN: el que está en sesión lo aplica y queda escrito.
        let r = api.ejecutar(Peticion {
            token: Some(admin.clone()),
            comando: Comando::CerrarConteo {
                conteo_id: id,
                aplicar: true,
                supervisor: None,
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "{r:?}");
        assert_eq!(stock_de(&api, "7501"), 96_000);
        // El pan no se contó: no se ajusta a cero por no haberse contado.
        assert_eq!(stock_de(&api, "7502"), 100_000);

        // Y quedó en la bitácora quién lo aplicó.
        let eventos = bitacora(&mut api, &admin);
        let aplicado = eventos
            .iter()
            .find(|e| e.accion == "cerrar_conteo")
            .expect("el conteo queda anotado");
        assert!(
            aplicado.detalle.contains("ajustes aplicados"),
            "{aplicado:?}"
        );
    }

    #[test]
    fn la_diferencia_es_contra_el_stock_de_ahora_no_contra_la_foto() {
        // Se abre el conteo, se vende, y luego se cuenta. La venta no debe
        // perderse: el ajuste se calcula contra el stock de ese momento.
        let mut api = escenario();
        let admin = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &admin, 0);
        let id = conteo(&mut api, &admin, Comando::IniciarConteo { categoria: None }).id;

        // Vendemos 10 coca-colas.
        let coca = api.buscar(&admin, "coca", 1).unwrap()[0].clone();
        let dto = VentaDto {
            lineas: vec![LineaDto {
                sku: coca.sku.clone(),
                descripcion: coca.name.clone(),
                qty_millis: 10_000,
                precio_cents: 0, // 0 = el precio del catálogo
                descuento_cents: 0,
            }],
            pagos: vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: 18_000,
                recibido_cents: 20_000,
            }],
            ..VentaDto::default()
        };
        api.cobrar(&admin, dto).unwrap();
        assert_eq!(stock_de(&api, "7501"), 90_000);

        // Se cuenta y resulta que sí había 90: coincide, no hay ajuste.
        let c = conteo(
            &mut api,
            &admin,
            Comando::ContarProducto {
                codigo: "7501".into(),
                cantidad_millis: 90_000,
            },
        );
        let coca = c.renglones.iter().find(|r| r.sku == "7501").unwrap();
        assert_eq!(coca.esperado_millis, 100_000, "la foto dice 100");
        assert_eq!(coca.actual_millis, 90_000, "pero ahora hay 90");
        assert_eq!(coca.diferencia_millis, Some(0), "no se ajusta nada");

        api.ejecutar(Peticion {
            token: Some(admin),
            comando: Comando::CerrarConteo {
                conteo_id: id,
                aplicar: true,
                supervisor: Some(CredencialesDto {
                    username: "jefe".into(),
                    pin: "2222".into(),
                }),
            },
        });
        assert_eq!(stock_de(&api, "7501"), 90_000);
    }

    #[test]
    fn el_conteo_se_puede_filtrar_por_categoria() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let id = Products::by_sku(api.db.conn(), "7501").unwrap().unwrap().id;
        let cat = Categories::asegurar(api.db.conn(), "Bebidas").unwrap();
        // Ojo: en las pruebas hay un `asignar` de las denominaciones del
        // arqueo, así que aquí se llama con la ruta completa.
        pos_db::categorias::asignar(api.db.conn(), id, Some(cat)).unwrap();

        let c = conteo(
            &mut api,
            &token,
            Comando::IniciarConteo {
                categoria: Some("Bebidas".into()),
            },
        );
        assert_eq!(c.productos, 1);
        assert_eq!(c.filtro, "Bebidas");
        assert_eq!(c.renglones[0].sku, "7501");
    }

    #[test]
    fn cancelar_el_conteo_libera_el_lugar_y_no_deja_rastro() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let id = conteo(&mut api, &token, Comando::IniciarConteo { categoria: None }).id;
        conteo(
            &mut api,
            &token,
            Comando::ContarProducto {
                codigo: "7501".into(),
                cantidad_millis: 96_000,
            },
        );
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::CancelarConteo { conteo_id: id },
        });
        assert!(matches!(r, Respuesta::Ok(_)), "{r:?}");
        // Se puede abrir otro: el lugar quedó libre.
        let c = conteo(&mut api, &token, Comando::IniciarConteo { categoria: None });
        assert_eq!(c.contados, 0);
        assert_eq!(
            c.productos, 2,
            "empieza de cero, sin lo que ya se había contado"
        );
        let _ = id;
        // Y el inventario intacto.
        assert_eq!(stock_de(&api, "7501"), 100_000);
    }

    #[test]
    fn un_cajero_no_puede_contar_el_inventario() {
        let mut api = escenario();
        let ana = entrar(&mut api, "ana", "4321");
        let e = error_de(&mut api, &ana, Comando::IniciarConteo { categoria: None });
        assert!(e.contains("no tiene permiso"), "{e}");
    }

    // =====================================================================
    // Importador con mapeo a mano
    // =====================================================================

    fn analisis(api: &mut LocalApi, token: &str, texto: &str) -> AnalisisCsvDto {
        match api.ejecutar(Peticion {
            token: Some(token.into()),
            comando: Comando::AnalizarCsv {
                texto: texto.into(),
            },
        }) {
            Respuesta::Ok(Cuerpo::AnalisisCsv(a)) => a,
            otro => panic!("análisis: {otro:?}"),
        }
    }

    #[test]
    fn el_dialogo_de_mapeo_dice_que_columna_es_cual() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let a = analisis(
            &mut api,
            &token,
            "Descripcion,Precio,Costo,Categoria\nCoca,18.50,12.00,Abarrotes\n",
        );
        assert_eq!(
            a.encabezados,
            vec!["Descripcion", "Precio", "Costo", "Categoria"]
        );
        assert_eq!(a.sugerido, vec!["nombre", "precio", "costo", "categoria"]);
        assert!(a.tiene_nombre);
        assert_eq!(a.renglones, 1);
        // La lista de campos viene con el análisis: la interfaz no la inventa.
        assert_eq!(a.campos.len(), 11);
        assert!(a.campos.iter().any(|(c, _)| c == "ignorar"));
    }

    #[test]
    fn importar_con_mapeo_a_mano_gana_y_asigna_la_categoria() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let csv = "c1,c2,c3\n18.50,Jabón Roma,Aseo\n15.00,Frijol 1kg,Abarrotes\n";
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::ImportarCsv {
                texto: csv.into(),
                actualizar: false,
                mapeo: vec!["precio".into(), "nombre".into(), "categoria".into()],
            },
        });
        let Respuesta::Ok(Cuerpo::Importe(r)) = r else {
            panic!("importación: falló");
        };
        assert_eq!(r.creados, 2, "{:?}", r.errores);

        // Se guardó con el nombre y el precio correctos, no al revés.
        let jabon = api.buscar(&token, "jabón", 5).unwrap();
        assert_eq!(jabon.len(), 1);
        assert_eq!(jabon[0].price_cents, 1850);

        // Las dos categorías del CSV existen y cada una tiene su producto.
        let cats = api.categorias(&token).unwrap();
        let abarrotes = cats
            .iter()
            .find(|c| c.name == "Abarrotes")
            .expect("se creó la categoría Abarrotes");
        let aseo = cats
            .iter()
            .find(|c| c.name == "Aseo")
            .expect("se creó la categoría Aseo");
        assert_eq!(abarrotes.productos, 1);
        assert_eq!(aseo.productos, 1);
    }

    #[test]
    fn mapear_una_columna_como_ignorar_la_deja_fuera() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let csv = "Nombre,Precio,Notas\nCoca,18.50,no surtir\n";
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::ImportarCsv {
                texto: csv.into(),
                actualizar: false,
                mapeo: vec!["nombre".into(), "precio".into(), "ignorar".into()],
            },
        });
        let Respuesta::Ok(Cuerpo::Importe(r)) = r else {
            panic!("importación: falló");
        };
        assert_eq!(r.creados, 1);
        assert!(r.errores.is_empty());
        let coca = api.buscar(&token, "coca", 5).unwrap();
        assert_eq!(coca[0].price_cents, 1850);
    }

    #[test]
    fn importar_sin_mapeo_sigue_funcionando_igual_que_antes() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::ImportarCsv {
                texto: "Nombre,Precio\nCoca,18.50\n".into(),
                actualizar: false,
                mapeo: Vec::new(),
            },
        });
        let Respuesta::Ok(Cuerpo::Importe(r)) = r else {
            panic!("importación: falló");
        };
        assert_eq!(r.creados, 1);
        assert!(!api.buscar(&token, "coca", 5).unwrap().is_empty());
    }

    // =====================================================================
    // Escritorio: reimprimir, reportes por cajero, categorías, respaldo
    // =====================================================================

    #[test]
    fn se_puede_reimprimir_un_ticket_por_folio() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &token, 50_000);
        api.cobrar(&token, venta("7501", 3, 1_800)).unwrap();

        let ticket = api.ticket_por_folio(&token, "000001").unwrap();
        assert_eq!(ticket.folio, "000001");
        assert_eq!(ticket.cajero, "Administrador");
        assert_eq!(ticket.total_cents, 5_400);
        assert_eq!(ticket.lineas.len(), 1);
        assert_eq!(ticket.lineas[0].descripcion, "Coca-Cola 600 ml");
        assert_eq!(ticket.lineas[0].qty_millis, 3_000);
        assert_eq!(ticket.pagos[0].metodo, "efectivo");
        assert_eq!(ticket.cambio_cents, 0, "el helper paga exacto");

        assert!(api.ticket_por_folio(&token, "999999").is_err());
    }

    #[test]
    fn un_ticket_guarda_el_cambio_y_los_pagos_mixtos() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &token, 50_000);

        // $54 de compra: $30 en efectivo (paga con $50) y $24 con tarjeta.
        let dto = VentaDto {
            lineas: vec![LineaDto {
                sku: "7501".into(),
                descripcion: "Coca-Cola".into(),
                qty_millis: 3_000,
                precio_cents: 1_800,
                descuento_cents: 0,
            }],
            descuento_global_cents: 0,
            pagos: vec![
                PagoDto {
                    metodo: "efectivo".into(),
                    monto_cents: 3_000,
                    recibido_cents: 5_000,
                },
                PagoDto {
                    metodo: "tarjeta".into(),
                    monto_cents: 2_400,
                    recibido_cents: 0,
                },
            ],
            cliente_id: None,
            nota: String::new(),
            supervisor: None,
        };
        let guardada = api.cobrar(&token, dto).unwrap();
        assert_eq!(guardada.total_cents, 5_400);
        assert_eq!(guardada.cambio_cents, 2_000, "pagó $50 por $30 en efectivo");

        // El efectivo del cajón sólo cuenta los $30, no los $50 recibidos.
        let caja = api.estado_caja(&token).unwrap();
        assert_eq!(caja.ventas_efectivo_cents, 3_000);

        let ticket = api.ticket_por_folio(&token, "000001").unwrap();
        assert_eq!(ticket.pagos.len(), 2);
        assert_eq!(ticket.cambio_cents, 2_000);
    }

    #[test]
    fn un_pago_incompleto_no_se_cobra() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &token, 0);
        let dto = VentaDto {
            lineas: vec![LineaDto {
                sku: "7501".into(),
                descripcion: "Coca-Cola".into(),
                qty_millis: 1_000,
                precio_cents: 1_800,
                descuento_cents: 0,
            }],
            descuento_global_cents: 0,
            pagos: vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: 1_000,
                recibido_cents: 1_000,
            }],
            cliente_id: None,
            nota: String::new(),
            supervisor: None,
        };
        assert!(api.cobrar(&token, dto).is_err());
        assert_eq!(api.totales_hoy(&token).unwrap().count, 0);
    }

    #[test]
    fn el_reporte_por_cajero_separa_quien_vendio_que() {
        let mut api = escenario();
        let t_admin = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &t_admin, 10_000);
        api.cobrar(&t_admin, venta("7501", 1, 1_800)).unwrap();

        let t_ana = entrar(&mut api, "ana", "4321");
        api.cobrar(&t_ana, venta("7502", 2, 1_850)).unwrap();

        let cajeros = api.totales_por_cajero(&t_admin).unwrap();
        assert_eq!(cajeros.len(), 2, "dos cajeros vendieron");
        let total: i64 = cajeros.iter().map(|c| c.vendido_cents).sum();
        assert_eq!(total, 1_800 + 3_700);
        assert!(cajeros.iter().all(|c| c.tickets == 1));
    }

    #[test]
    fn las_ventas_se_exportan_a_csv() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &token, 0);
        api.cobrar(&token, venta("7501", 2, 1_800)).unwrap();

        let csv = match api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::ExportarVentasCsv {
                desde: String::new(),
                hasta: String::new(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Texto(t)) => t,
            otro => panic!("esperaba el CSV: {otro:?}"),
        };
        assert!(csv.starts_with(
            "folio,fecha,cajero,subtotal,descuento,impuesto,total,pagos,cambio,estado"
        ));
        let renglones: Vec<&str> = csv.lines().collect();
        assert_eq!(renglones.len(), 2); // encabezado + una venta
        assert!(renglones[1].starts_with("000001,"));
        assert!(renglones[1].contains("36.00")); // 2 x $18.00
    }

    #[test]
    fn las_categorias_se_crean_y_se_cuentan() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");

        let r = api.categorias(&token).unwrap();
        assert!(r.is_empty(), "empieza sin categorías");

        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarCategoria {
                nombre: "Abarrotes".into(),
            },
        });
        // Crearla dos veces no la duplica.
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarCategoria {
                nombre: "abarrotes".into(),
            },
        });
        let r = api.categorias(&token).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "Abarrotes");

        // Un producto nuevo con categoría queda dentro de ella.
        let dto = NuevoProductoDto {
            id: None,
            sku: "9001".into(),
            name: "Frijol".into(),
            price_cents: 2_900,
            cost_cents: 2_000,
            tax_bp: 1_600,
            unit: "pza".into(),
            stock_millis: 10_000,
            min_stock_millis: 0,
            track_inventory: true,
            barcodes: vec![],
            categoria: Some("Abarrotes".into()),
            clave_sat: None,
            clave_unidad: None,
            unidad_sat: None,
        };
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarProducto { producto: dto },
        });
        assert_eq!(
            api.categorias(&token).unwrap()[0].productos,
            1,
            "al dar de alta con categoría debe contarlo"
        );

        // Editar un producto existente también le pone (y le quita) categoría.
        let existente = api.buscar(&token, "coca", 1).unwrap()[0].clone();
        let mut edicion = NuevoProductoDto {
            id: Some(existente.id),
            sku: existente.sku.clone(),
            name: existente.name.clone(),
            price_cents: existente.price_cents,
            cost_cents: existente.cost_cents,
            tax_bp: 1_600,
            unit: "pza".into(),
            stock_millis: 0,
            min_stock_millis: 0,
            track_inventory: true,
            barcodes: vec![],
            categoria: Some("Abarrotes".into()),
            clave_sat: None,
            clave_unidad: None,
            unidad_sat: None,
        };
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarProducto {
                producto: edicion.clone(),
            },
        });
        assert_eq!(
            api.categorias(&token).unwrap()[0].productos,
            2,
            "la edición debe asignar la categoría"
        );

        // Sin categoría, se queda fuera.
        edicion.categoria = None;
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarProducto { producto: edicion },
        });
        assert_eq!(api.categorias(&token).unwrap()[0].productos, 1);
    }

    #[test]
    fn el_respaldo_deja_una_copia_que_abre() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &token, 0);
        api.cobrar(&token, venta("7501", 1, 1_800)).unwrap();

        let dir = std::env::temp_dir().join(format!("pos-api-respaldo-{}", ahora_nanos()));
        let ruta = dir.join("copia.db");
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::Respaldo {
                ruta: ruta.display().to_string(),
            },
        });
        assert!(r.es_ok(), "{r:?}");
        assert!(ruta.exists());

        // La copia abre y trae el historial.
        let copia = LocalApi::new(Database::open(&ruta).unwrap());
        let token2 = entrar(
            &mut LocalApi::new(Database::open(&ruta).unwrap()),
            "admin",
            "1234",
        );
        let _ = copia;
        let totales = api.totales_hoy(&token).unwrap();
        assert_eq!(totales.count, 1);
        let _ = token2;

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_cajero_no_puede_sacar_el_respaldo() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        let r = api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::Respaldo {
                ruta: "/tmp/no-debe-crearse.db".into(),
            },
        });
        assert!(r.es_error());
        assert!(!std::path::Path::new("/tmp/no-debe-crearse.db").exists());
    }

    #[test]
    fn los_productos_bajo_minimo_aparecen_en_la_alerta() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let id = api.buscar(&token, "coca", 1).unwrap()[0].id;

        // El mínimo se sube por encima del stock.
        let dto = NuevoProductoDto {
            id: Some(id),
            sku: "7501".into(),
            name: "Coca-Cola 600 ml".into(),
            price_cents: 1_800,
            cost_cents: 900,
            tax_bp: 1_600,
            unit: "pza".into(),
            stock_millis: 0,
            min_stock_millis: 999_000,
            track_inventory: true,
            barcodes: vec![],
            categoria: None,
            clave_sat: None,
            clave_unidad: None,
            unidad_sat: None,
        };
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarProducto { producto: dto },
        });

        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::ProductosBajoMinimo,
        }) {
            Respuesta::Ok(Cuerpo::Productos(p)) => {
                assert_eq!(p.len(), 1);
                assert_eq!(p[0].sku, "7501");
            }
            otro => panic!("esperaba productos: {otro:?}"),
        }
    }

    fn ahora_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    #[test]
    fn el_inventario_se_descuenta_al_vender_por_api() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &token, 0);
        api.cobrar(&token, venta("7501", 4, 1800)).unwrap();

        let p = Products::by_sku(api.db.conn(), "7501").unwrap().unwrap();
        assert_eq!(p.stock_millis, 96_000); // 100 - 4
    }

    fn dejar_stock(api: &mut LocalApi, sku: &str, piezas: i64) {
        let p = Products::by_sku(api.db.conn(), sku).unwrap().unwrap();
        let delta = piezas * 1_000 - p.stock_millis;
        if delta != 0 {
            Products::adjust_stock(api.db.conn_mut(), p.id, delta, "prueba", 1).unwrap();
        }
    }

    #[test]
    fn vender_30_cuando_hay_29_se_rechaza_y_el_stock_sigue_igual() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        dejar_stock(&mut api, "7501", 29);

        let r = api.cobrar(&t, venta("7501", 30, 1800));
        assert!(r.is_err(), "no se puede vender lo que no hay: {r:?}");
        let msg = r.unwrap_err();
        assert!(
            msg.contains("29") && msg.contains("30"),
            "el error dice cuánto hay y cuánto se pide: {msg}"
        );
        assert_eq!(stock_de(&api, "7501"), 29_000);
    }

    #[test]
    fn vender_justo_las_29_deja_el_anaquel_en_cero() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        dejar_stock(&mut api, "7501", 29);
        api.cobrar(&t, venta("7501", 29, 1800)).unwrap();
        assert_eq!(stock_de(&api, "7501"), 0);
        assert!(
            api.cobrar(&t, venta("7501", 1, 1800)).is_err(),
            "con el anaquel vacío no se vende una más"
        );
        assert_eq!(stock_de(&api, "7501"), 0);
    }

    #[test]
    fn dos_renglones_del_mismo_producto_no_pueden_sumar_mas_que_el_stock() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        dejar_stock(&mut api, "7501", 29);
        // 20 a precio de lista y 10 a precio distinto: son dos renglones.
        let dto = VentaDto {
            lineas: vec![
                LineaDto {
                    sku: "7501".into(),
                    descripcion: "coca".into(),
                    qty_millis: 20_000,
                    precio_cents: 1_800,
                    descuento_cents: 0,
                },
                LineaDto {
                    sku: "7501".into(),
                    descripcion: "coca".into(),
                    qty_millis: 10_000,
                    precio_cents: 1_500,
                    descuento_cents: 0,
                },
            ],
            pagos: vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: 20 * 1_800 + 10 * 1_500,
                recibido_cents: 20 * 1_800 + 10 * 1_500,
            }],
            ..Default::default()
        };
        assert!(api.cobrar(&t, dto).is_err());
        assert_eq!(stock_de(&api, "7501"), 29_000);
    }

    #[test]
    fn un_servicio_sin_inventario_si_se_puede_vender_sin_stock() {
        let mut api = escenario();
        let t = entrar(&mut api, "admin", "1234");
        abrir_caja(&mut api, &t, 0);
        Products::insert(
            api.db.conn_mut(),
            &NewProduct {
                sku: "SERV".into(),
                name: "Recarga".into(),
                price_cents: 1_000,
                cost_cents: 0,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: 0,
                min_stock_millis: 0,
                track_inventory: false,
                barcodes: vec![],
            },
        )
        .unwrap();
        let r = api.cobrar(&t, venta("SERV", 5, 1_000));
        assert!(r.is_ok(), "un servicio no se acaba: {r:?}");
        assert_eq!(
            Products::by_sku(api.db.conn(), "SERV")
                .unwrap()
                .unwrap()
                .stock_millis,
            0
        );
    }

    #[test]
    fn subir_el_precio_tambien_queda_anotado() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        let t_jefe = entrar(&mut api, "jefe", "2222");
        abrir_caja(&mut api, &t_ana, 0);
        api.cobrar(&t_ana, venta("7501", 1, 2_000)).unwrap();
        let eventos: Vec<_> = bitacora(&mut api, &t_jefe)
            .into_iter()
            .filter(|e| e.accion == "precio")
            .collect();
        assert_eq!(eventos.len(), 1);
        assert!(
            eventos[0].detalle.contains("20.00") && eventos[0].detalle.contains("18.00"),
            "de lista a cobrado: {}",
            eventos[0].detalle
        );
    }

    #[test]
    fn precio_a_mano_y_descuento_ambos_sin_pin_y_ambos_anotados() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        let mut dto = venta("7501", 1, 1_500);
        dto.lineas[0].descuento_cents = 100;
        dto.pagos[0].monto_cents = 1_400;
        dto.pagos[0].recibido_cents = 1_400;
        let ok = api.cobrar(&t, dto).unwrap();
        assert_eq!(ok.total_cents, 1_400);
        let t_jefe = entrar(&mut api, "jefe", "2222");
        let bit = bitacora(&mut api, &t_jefe);
        assert!(bit.iter().any(|e| e.accion == "precio"));
        assert!(bit.iter().any(|e| e.accion == "descuento"));
        assert!(bit.iter().all(|e| e.accion != "autorizar"));
    }

    #[test]
    fn una_venta_vacia_se_rechaza() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        let dto = VentaDto {
            lineas: vec![],
            pagos: vec![],
            ..Default::default()
        };
        assert!(api.cobrar(&t, dto).is_err());
    }

    #[test]
    fn una_linea_en_cero_se_rechaza() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        let dto = VentaDto {
            lineas: vec![LineaDto {
                sku: "7501".into(),
                descripcion: "coca".into(),
                qty_millis: 0,
                precio_cents: 1_800,
                descuento_cents: 0,
            }],
            pagos: vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: 0,
                recibido_cents: 0,
            }],
            ..Default::default()
        };
        assert!(api.cobrar(&t, dto).is_err());
        assert_eq!(stock_de(&api, "7501"), 100_000);
    }

    #[test]
    fn un_sku_que_no_existe_no_toca_el_inventario() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        assert!(api.cobrar(&t, venta("NOEXISTE", 1, 100)).is_err());
        assert_eq!(stock_de(&api, "7501"), 100_000);
        assert_eq!(stock_de(&api, "7502"), 100_000);
    }

    #[test]
    fn dos_productos_distintos_descuentan_cada_quien_el_suyo() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        dejar_stock(&mut api, "7501", 10);
        dejar_stock(&mut api, "7502", 5);
        let dto = VentaDto {
            lineas: vec![
                LineaDto {
                    sku: "7501".into(),
                    descripcion: "coca".into(),
                    qty_millis: 3_000,
                    precio_cents: 1_800,
                    descuento_cents: 0,
                },
                LineaDto {
                    sku: "7502".into(),
                    descripcion: "pan".into(),
                    qty_millis: 2_000,
                    precio_cents: 1_850,
                    descuento_cents: 0,
                },
            ],
            pagos: vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: 3 * 1_800 + 2 * 1_850,
                recibido_cents: 3 * 1_800 + 2 * 1_850,
            }],
            ..Default::default()
        };
        api.cobrar(&t, dto).unwrap();
        assert_eq!(stock_de(&api, "7501"), 7_000);
        assert_eq!(stock_de(&api, "7502"), 3_000);
    }

    #[test]
    fn si_uno_no_alcanza_el_otro_tampoco_se_descuenta() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        dejar_stock(&mut api, "7501", 5);
        dejar_stock(&mut api, "7502", 5);
        let dto = VentaDto {
            lineas: vec![
                LineaDto {
                    sku: "7501".into(),
                    descripcion: "coca".into(),
                    qty_millis: 6_000,
                    precio_cents: 1_800,
                    descuento_cents: 0,
                },
                LineaDto {
                    sku: "7502".into(),
                    descripcion: "pan".into(),
                    qty_millis: 1_000,
                    precio_cents: 1_850,
                    descuento_cents: 0,
                },
            ],
            pagos: vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: 6 * 1_800 + 1_850,
                recibido_cents: 6 * 1_800 + 1_850,
            }],
            ..Default::default()
        };
        assert!(api.cobrar(&t, dto).is_err());
        assert_eq!(stock_de(&api, "7501"), 5_000);
        assert_eq!(stock_de(&api, "7502"), 5_000);
    }

    #[test]
    fn veinte_mas_nueve_cuando_hay_29_si_pasa() {
        let mut api = escenario();
        let t = entrar(&mut api, "ana", "4321");
        abrir_caja(&mut api, &t, 0);
        dejar_stock(&mut api, "7501", 29);
        // Dos renglones del mismo SKU: se suman contra el anaquel.
        let dto = VentaDto {
            lineas: vec![
                LineaDto {
                    sku: "7501".into(),
                    descripcion: "coca".into(),
                    qty_millis: 20_000,
                    precio_cents: 1_800,
                    descuento_cents: 0,
                },
                LineaDto {
                    sku: "7501".into(),
                    descripcion: "coca".into(),
                    qty_millis: 9_000,
                    precio_cents: 1_800,
                    descuento_cents: 0,
                },
            ],
            pagos: vec![PagoDto {
                metodo: "efectivo".into(),
                monto_cents: 29 * 1_800,
                recibido_cents: 29 * 1_800,
            }],
            ..Default::default()
        };
        api.cobrar(&t, dto).unwrap();
        assert_eq!(stock_de(&api, "7501"), 0);
    }

    #[test]
    fn vender_al_precio_de_lista_no_anota_un_cambio() {
        let mut api = escenario();
        let t_ana = entrar(&mut api, "ana", "4321");
        let t_jefe = entrar(&mut api, "jefe", "2222");
        abrir_caja(&mut api, &t_ana, 0);
        api.cobrar(&t_ana, venta("7501", 1, 1_800)).unwrap();
        assert!(
            bitacora(&mut api, &t_jefe)
                .iter()
                .all(|e| e.accion != "precio"),
            "el precio de lista no es un cambio"
        );
    }

    #[test]
    fn la_configuracion_viaja_como_dto() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let r = api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::GuardarConfig {
                config: ConfigDto {
                    tienda: "Abarrotes Lupita".into(),
                    direccion: "Av. Hidalgo 123".into(),
                    pie: "Gracias".into(),
                    impresora: "/dev/usb/lp0".into(),
                    ancho_ticket: 32,
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
            },
        });
        assert!(matches!(r, Respuesta::Ok(_)));

        let r = api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::LeerConfig,
        });
        match r {
            Respuesta::Ok(Cuerpo::Configuracion(c)) => {
                assert_eq!(c.tienda, "Abarrotes Lupita");
                assert_eq!(c.ancho_ticket, 32);
            }
            otro => panic!("config: {otro:?}"),
        }
    }

    #[test]
    fn la_impresora_se_configura_y_se_lee_completa() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        let config = ConfigDto {
            tienda: "Abarrotes Lupita".into(),
            direccion: String::new(),
            pie: "Gracias".into(),
            impresora: "/dev/ttyUSB0".into(),
            ancho_ticket: 32,
            impresora_perfil: "Epson TM (T20/T82/T88)".into(),
            impresora_baudios: 115200,
            abrir_cajon: false,
            respaldo_automatico: true,
            respaldo_carpeta: String::new(),
            respaldo_cada_dias: 1,
            respaldos_a_conservar: 14,
            respaldar_al_cortar: true,
            puerto_lan: 8080,
        };
        assert!(matches!(
            api.ejecutar(Peticion {
                token: Some(token.clone()),
                comando: Comando::GuardarConfig {
                    config: config.clone()
                },
            }),
            Respuesta::Ok(_)
        ));

        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::LeerConfig,
        }) {
            Respuesta::Ok(Cuerpo::Configuracion(c)) => {
                assert_eq!(c.impresora, "/dev/ttyUSB0");
                assert_eq!(c.impresora_perfil, "Epson TM (T20/T82/T88)");
                assert_eq!(c.impresora_baudios, 115200);
                assert!(!c.abrir_cajon, "el cajón quedó activado");
                assert_eq!(c.ancho_ticket, 32);
            }
            otro => panic!("config: {otro:?}"),
        }
    }

    #[test]
    fn la_prueba_de_impresion_deja_un_ticket_de_verdad() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        // Se "imprime" a un archivo: es la misma ruta de código que a la USB.
        let ruta = std::env::temp_dir().join("pos-api-prueba-impresion.bin");
        let _ = std::fs::remove_file(&ruta);
        std::fs::write(&ruta, b"").unwrap();
        let ruta_txt = ruta.to_str().unwrap().to_string();

        match api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::ProbarImpresion {
                ruta: ruta_txt.clone(),
                baudios: 9600,
                perfil: "Genérica 80 mm".into(),
                abrir_cajon: true,
            },
        }) {
            Respuesta::Ok(Cuerpo::Prueba(p)) => {
                assert!(p.ok, "la prueba falló: {}", p.mensaje);
                assert!(p.bytes > 100, "ticket sospechosamente corto: {}", p.bytes);
                assert!(p.mensaje.contains("¿Salió completo?"), "{}", p.mensaje);
                assert!(p.mensaje.contains("Quedó guardada"), "{}", p.mensaje);
            }
            otro => panic!("prueba: {otro:?}"),
        }

        match api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::LeerConfig,
        }) {
            Respuesta::Ok(Cuerpo::Configuracion(c)) => {
                assert_eq!(c.impresora, ruta_txt, "la prueba que sí salió se guarda");
            }
            otro => panic!("config tras prueba: {otro:?}"),
        }

        let bytes = std::fs::read(&ruta).unwrap();
        // Empieza con ESC @, trae los acentos y termina con el corte.
        assert_eq!(&bytes[0..2], &[0x1B, b'@']);
        assert!(
            bytes.windows(3).any(|w| w == [0x1D, b'V', b'1']),
            "sin corte"
        );
        assert!(
            bytes.windows(5).any(|w| w == [0x1B, b'p', 0, 25, 250]),
            "sin pulso al cajón"
        );
        // Y el nombre de la tienda va impreso en la primera línea.
        let texto = pos_hw::decodificar(&pos_hw::strip_commands(&bytes), pos_hw::Pagina::Cp850);
        let tienda = match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::LeerConfig,
        }) {
            Respuesta::Ok(Cuerpo::Configuracion(c)) => c.tienda,
            otro => panic!("config: {otro:?}"),
        };
        assert_eq!(
            texto.lines().next().unwrap_or("").trim(),
            tienda,
            "el encabezado no es el nombre de la tienda: {texto}"
        );
        std::fs::remove_file(&ruta).unwrap();
    }

    #[test]
    fn una_impresora_que_no_existe_avisa_sin_romper_nada() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::ProbarImpresion {
                ruta: "/dev/usb/lpQueNoExiste".into(),
                baudios: 9600,
                perfil: "Genérica 80 mm".into(),
                abrir_cajon: false,
            },
        }) {
            Respuesta::Ok(Cuerpo::Prueba(p)) => {
                assert!(!p.ok);
                assert!(p.mensaje.contains("No se pudo imprimir"), "{}", p.mensaje);
                assert!(
                    p.mensaje.contains("encendida"),
                    "sin consejo: {}",
                    p.mensaje
                );
                assert!(
                    p.mensaje.contains("impresoras") || p.mensaje.contains("Detectar"),
                    "sin pista de cómo listar: {}",
                    p.mensaje
                );
            }
            otro => panic!("prueba: {otro:?}"),
        }
    }

    #[test]
    fn listar_impresoras_y_probar_exigen_permiso_de_configuracion() {
        let mut api = escenario();
        let token = entrar(&mut api, "ana", "4321");
        for comando in [
            Comando::Impresoras,
            Comando::ProbarImpresion {
                ruta: "/dev/usb/lp0".into(),
                baudios: 9600,
                perfil: "Genérica 80 mm".into(),
                abrir_cajon: false,
            },
        ] {
            assert!(
                !matches!(
                    api.ejecutar(Peticion {
                        token: Some(token.clone()),
                        comando,
                    }),
                    Respuesta::Ok(_)
                ),
                "la cajera no debe poder tocar la impresora"
            );
        }
    }

    #[test]
    fn el_listado_de_impresoras_responde_aunque_no_haya_niguna() {
        let mut api = escenario();
        let token = entrar(&mut api, "admin", "1234");
        match api.ejecutar(Peticion {
            token: Some(token),
            comando: Comando::Impresoras,
        }) {
            // En la máquina de desarrollo no hay impresora: debe responder
            // con una lista vacía, no con un error.
            Respuesta::Ok(Cuerpo::Impresoras(lista)) => {
                for i in &lista {
                    assert!(!i.ruta.is_empty());
                    assert!(!i.perfil.is_empty());
                    assert!(matches!(
                        i.transporte.as_str(),
                        "usb" | "serie" | "red" | "windows"
                    ));
                }
            }
            otro => panic!("impresoras: {otro:?}"),
        }
    }

    #[test]
    fn pedir_y_recibir_sube_el_inventario_por_la_api() {
        let mut api = escenario();
        let t = entrar(&mut api, "admin", "1234");
        let t_ana = entrar(&mut api, "ana", "4321");
        let stock_antes = stock_de(&api, "7501");

        // El cajero no pide mercancía.
        match api.ejecutar(Peticion {
            token: Some(t_ana),
            comando: Comando::Proveedores {
                termino: String::new(),
            },
        }) {
            Respuesta::Error(e) => assert!(e.contains("inventario"), "{e}"),
            otro => panic!("el cajero no debe ver proveedores: {otro:?}"),
        }

        let id_prov = match api.ejecutar(Peticion {
            token: Some(t.clone()),
            comando: Comando::GuardarProveedor {
                proveedor: NuevoProveedorDto {
                    id: None,
                    name: "Bebidas del Valle".into(),
                    phone: "555".into(),
                    nota: String::new(),
                },
            },
        }) {
            Respuesta::Ok(Cuerpo::Texto(s)) => s.parse::<i64>().unwrap(),
            otro => panic!("proveedor: {otro:?}"),
        };

        let pedido = match api.ejecutar(Peticion {
            token: Some(t.clone()),
            comando: Comando::NuevoPedido {
                proveedor_id: id_prov,
                nota: String::new(),
            },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => p,
            otro => panic!("nuevo pedido: {otro:?}"),
        };
        assert_eq!(pedido.folio, "P-000001");

        let pedido = match api.ejecutar(Peticion {
            token: Some(t.clone()),
            comando: Comando::AgregarAPedido {
                pedido_id: pedido.id,
                codigo: "7501".into(),
                qty_millis: 6_000,
                costo_cents: Some(1_000),
            },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => p,
            otro => panic!("agregar: {otro:?}"),
        };
        let product_id = pedido.lineas[0].product_id;

        let pedido = match api.ejecutar(Peticion {
            token: Some(t.clone()),
            comando: Comando::RecibirPedido {
                id: pedido.id,
                entregas: vec![EntregaDto {
                    product_id,
                    qty_millis: 6_000,
                }],
            },
        }) {
            Respuesta::Ok(Cuerpo::Pedido(p)) => p,
            otro => panic!("recibir: {otro:?}"),
        };
        assert_eq!(pedido.estado, "recibido");
        assert_eq!(stock_de(&api, "7501"), stock_antes + 6_000);
    }
}
