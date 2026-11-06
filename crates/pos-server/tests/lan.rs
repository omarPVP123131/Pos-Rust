//! Prueba de integración: **el día completo de operación por la LAN**.
//!
//! No usa nada de `pos-ui`. Llama a los mismos comandos que la interfaz de
//! escritorio, pero a través de `HttpApi`, que es exactamente lo que haría la
//! app móvil en el celular del dueño.
//!
//! Si esta prueba pasa, está demostrado que:
//!   1. La lógica de negocio NO vive en la interfaz.
//!   2. Un cliente nuevo (móvil, web, otra caja) sólo necesita HTTP.
//!   3. Cambiar de motor gráfico no toca nada de esto.

use pos_api::{
    Comando, Cuerpo, HttpApi, LineaDto, LocalApi, NuevoClienteDto, PagoDto, Peticion, PosApi,
    Respuesta, VentaDto,
};
use pos_core::Role;
use pos_db::products::{NewProduct, Products};
use pos_db::users::Users;
use pos_db::Database;
use pos_server::servir_en_puerto_libre;

/// Monta un punto de venta con usuarios y catálogo, y lo deja escuchando.
fn escenario_en_lan() -> HttpApi {
    let mut db = Database::open_in_memory().unwrap();
    Users::crear(db.conn_mut(), "admin", "Administrador", "1234", Role::Admin).unwrap();
    Users::crear(db.conn_mut(), "jefe", "El Jefe", "2222", Role::Supervisor).unwrap();
    Users::crear(db.conn_mut(), "ana", "Ana", "4321", Role::Cajero).unwrap();
    for (sku, nombre, precio) in [
        ("7501", "Coca-Cola 600 ml", 1800),
        ("7502", "Pan blanco", 1850),
    ] {
        Products::insert(
            db.conn_mut(),
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
                barcodes: vec![format!("{sku}0000000")],
            },
        )
        .unwrap();
    }

    let api: std::sync::Arc<std::sync::Mutex<Box<dyn PosApi>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Box::new(LocalApi::new(db))));
    let puerto = servir_en_puerto_libre(api, "127.0.0.1").unwrap();
    HttpApi::new(format!("http://127.0.0.1:{puerto}"))
}

fn ejecutar(api: &mut HttpApi, token: Option<&str>, comando: Comando) -> Cuerpo {
    match api.ejecutar(Peticion {
        token: token.map(|t| t.to_string()),
        comando,
    }) {
        Respuesta::Ok(c) => c,
        Respuesta::Error(e) => panic!("el servidor respondió error: {e}"),
    }
}

#[test]
fn el_dia_completo_por_red_con_la_misma_api_que_el_escritorio() {
    let mut api = escenario_en_lan();

    // 1. La pantalla de login pide la lista de usuarios: la pública, porque
    //    todavía no hay sesión.
    let usuarios = match ejecutar(&mut api, None, Comando::UsuariosLogin) {
        Cuerpo::Usuarios(u) => u,
        otro => panic!("esperaba usuarios, llegó {otro:?}"),
    };
    assert_eq!(usuarios.len(), 3);
    assert!(
        usuarios.iter().all(|u| u.role.is_empty()),
        "por la red tampoco se reparten los roles de la tienda"
    );

    // Y la lista completa no se da sino hasta que alguien entra.
    let r = api.ejecutar(Peticion {
        token: None,
        comando: Comando::Usuarios,
    });
    assert!(
        matches!(r, Respuesta::Error(_)),
        "sin sesión, por la red menos: {r:?}"
    );

    // 2. Entra la cajera.
    let sesion = api.login("ana", "4321").expect("login por red");
    let token = sesion.token;
    assert_eq!(sesion.usuario.display_name, "Ana");

    // 3. Sin caja abierta no se puede vender (regla que vive en el servidor).
    let venta = VentaDto {
        lineas: vec![LineaDto {
            sku: "7501".into(),
            descripcion: "Coca-Cola 600 ml".into(),
            qty_millis: 3000,
            precio_cents: 1800,
            descuento_cents: 0,
        }],
        pagos: vec![PagoDto {
            metodo: "efectivo".into(),
            monto_cents: 5400,
            recibido_cents: 6000,
        }],
        ..Default::default()
    };
    assert!(api.cobrar(&token, venta.clone()).is_err());

    // 4. Abre caja con fondo de $500.
    ejecutar(
        &mut api,
        Some(&token),
        Comando::AbrirCaja {
            fondo_cents: 50_000,
        },
    );

    // 5. Busca el producto escaneando su código de barras.
    let productos = api.buscar(&token, "coca", 10).unwrap();
    assert_eq!(productos.len(), 1);
    assert_eq!(productos[0].price_cents, 1800);

    // 6. Cobra: $54 y el cliente paga con $60.
    let vendida = api.cobrar(&token, venta).unwrap();
    assert_eq!(vendida.folio, "000001");
    assert_eq!(vendida.total_cents, 5400);
    assert_eq!(vendida.cambio_cents, 600);

    // 7. La caja refleja la venta (el cambio ya está descontado).
    let caja = api.estado_caja(&token).unwrap();
    assert!(caja.abierta);
    assert_eq!(caja.ventas_efectivo_cents, 5400);
    assert_eq!(caja.esperado_cents, 55_400);

    // 8. Un descuento lo hace el cajero: no pide PIN y queda anotado.
    let con_descuento = VentaDto {
        lineas: vec![LineaDto {
            sku: "7502".into(),
            descripcion: "Pan blanco".into(),
            qty_millis: 1000,
            precio_cents: 1850,
            descuento_cents: 0,
        }],
        pagos: vec![PagoDto {
            metodo: "efectivo".into(),
            monto_cents: 1350,
            recibido_cents: 1350,
        }],
        descuento_global_cents: 500,
        ..Default::default()
    };
    let segunda = api.cobrar(&token, con_descuento).unwrap();
    assert_eq!(segunda.total_cents, 1350);
    assert_eq!(segunda.descuento_cents, 500);

    // 9. Fiado: alta de cliente, venta a crédito y abono.
    let id_cliente = match ejecutar(
        &mut api,
        Some(&token),
        Comando::GuardarCliente {
            cliente: NuevoClienteDto {
                name: "Doña Mary".into(),
                phone: "5512345678".into(),
                credit_limit_cents: 30_000,
                ..Default::default()
            },
        },
    ) {
        Cuerpo::Texto(id) => id.parse::<i64>().unwrap(),
        otro => panic!("esperaba el id, llegó {otro:?}"),
    };

    let fiado = VentaDto {
        lineas: vec![LineaDto {
            sku: "7501".into(),
            descripcion: "Coca-Cola".into(),
            qty_millis: 5000,
            precio_cents: 1800,
            descuento_cents: 0,
        }],
        pagos: vec![PagoDto {
            metodo: "credito".into(),
            monto_cents: 9000,
            recibido_cents: 0,
        }],
        cliente_id: Some(id_cliente),
        ..Default::default()
    };
    api.cobrar(&token, fiado).unwrap();

    let clientes = api.clientes(&token, "Mary").unwrap();
    assert_eq!(clientes[0].saldo_cents, 9000);
    assert_eq!(clientes[0].disponible_cents, 21_000);

    ejecutar(
        &mut api,
        Some(&token),
        Comando::Abonar {
            cliente_id: id_cliente,
            monto_cents: 4000,
            metodo: "efectivo".into(),
        },
    );
    assert_eq!(api.clientes(&token, "Mary").unwrap()[0].saldo_cents, 5000);

    // 10. El fiado no toca el efectivo del cajón.
    let caja = api.estado_caja(&token).unwrap();
    assert_eq!(caja.ventas_efectivo_cents, 6750); // 54 + 13.50

    // 11. Totales del día por red.
    let totales = api.totales_hoy(&token).unwrap();
    assert_eq!(totales.count, 3);
    assert_eq!(totales.total_cents, 6750 + 9000);

    // 12. Cierra la caja: el arqueo no cuadra, se cierra igual y queda anotado.
    let mut piezas: Vec<(i64, i64)> = pos_core::DENOMINACIONES_MXN
        .iter()
        .map(|(d, _)| (*d, 0))
        .collect();
    asignar(&mut piezas, 50_000, 1); // $500
    asignar(&mut piezas, 10_000, 1); // $100
    asignar(&mut piezas, 5_000, 1); // $50
    asignar(&mut piezas, 200, 12); // $2.40 -> contado $652.40

    ejecutar(
        &mut api,
        Some(&token),
        Comando::CerrarCaja {
            piezas,
            supervisor: None,
        },
    );

    let caja = api.estado_caja(&token).unwrap();
    assert!(!caja.abierta);
}

fn asignar(piezas: &mut [(i64, i64)], denominacion: i64, cantidad: i64) {
    if let Some(p) = piezas.iter_mut().find(|(d, _)| *d == denominacion) {
        p.1 = cantidad;
    }
}

#[test]
fn dos_clientes_comparten_el_mismo_estado() {
    // Dos "dispositivos" distintos (dos HttpApi) contra el mismo servidor:
    // lo que vende uno lo ve el otro.
    let mut caja1 = escenario_en_lan();
    let mut caja2 = HttpApi::new(caja1.base());

    let t1 = caja1.login("ana", "4321").unwrap().token;
    ejecutar(
        &mut caja1,
        Some(&t1),
        Comando::AbrirCaja {
            fondo_cents: 10_000,
        },
    );

    let venta = VentaDto {
        lineas: vec![LineaDto {
            sku: "7502".into(),
            descripcion: "Pan".into(),
            qty_millis: 2000,
            precio_cents: 1850,
            descuento_cents: 0,
        }],
        pagos: vec![PagoDto {
            metodo: "tarjeta".into(),
            monto_cents: 3700,
            recibido_cents: 0,
        }],
        ..Default::default()
    };
    caja1.cobrar(&t1, venta).unwrap();

    // El segundo dispositivo ve la venta del primero.
    let t2 = caja2.login("admin", "1234").unwrap().token;
    let totales = caja2.totales_hoy(&t2).unwrap();
    assert_eq!(totales.count, 1);
    assert_eq!(totales.tarjeta_cents, 3700);
}
