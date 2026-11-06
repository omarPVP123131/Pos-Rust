//! Prueba de apagones: `kill -9` en el peor momento posible.
//!
//! Un mostrador no apaga la computadora: se va la luz, se cuelga el sistema o
//! alguien desconecta la regleta. Esta prueba hace exactamente eso —mata el
//! proceso con SIGKILL mientras está escribiendo una venta— y después revisa
//! la base dato por dato.
//!
//! Lo que se verifica tras cada apagón:
//!
//! 1. `PRAGMA integrity_check` responde "ok" (la base no quedó corrupta).
//! 2. La base abre y sigue siendo usable.
//! 3. **Ninguna venta quedó a medias**: no existe una venta sin renglones, ni
//!    una venta cuyos pagos no sumen su total. O está completa, o no está.
//! 4. El **inventario cuadra** contra el libro de movimientos.
//! 5. No hay pagos ni renglones huérfanos (apuntando a una venta inexistente).
//!
//! Corre con:
//! ```bash
//! cargo test --test apagon                       # 40 apagones (rápido)
//! POS_ITERACIONES=200 cargo test --test apagon   # los 200 del criterio CA8
//! ```

use pos_api::{Comando, HttpApi, LineaDto, PagoDto, Peticion, PosApi, VentaDto};
use pos_db::Database;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Un servidor del punto de venta al que luego vamos a matar sin piedad.
struct Servidor {
    proceso: Child,
    puerto: u16,
    base: PathBuf,
}

impl Servidor {
    fn nuevo(dir: &Path, puerto: u16) -> Servidor {
        let base = dir.join("tienda.db");
        let proceso = Command::new(env!("CARGO_BIN_EXE_pos"))
            .args([
                "--db",
                base.to_str().unwrap(),
                "--servir",
                "--puerto",
                &puerto.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("no pude arrancar el punto de venta");
        Servidor {
            proceso,
            puerto,
            base,
        }
    }

    fn api(&self) -> HttpApi {
        HttpApi::new(format!("http://127.0.0.1:{}", self.puerto))
    }

    /// Espera a que el servidor acepte conexiones.
    fn esperar(&self) -> bool {
        let inicio = Instant::now();
        while inicio.elapsed() < Duration::from_secs(20) {
            if TcpStream::connect(("127.0.0.1", self.puerto)).is_ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        false
    }

    /// **Apagón**: SIGKILL, sin avisar, como si se fuera la luz.
    fn matar(&mut self) {
        let _ = self.proceso.kill();
        let _ = self.proceso.wait();
    }
}

impl Drop for Servidor {
    fn drop(&mut self) {
        // Si el test termina antes, no dejamos procesos colgados.
        let _ = self.proceso.kill();
        let _ = self.proceso.wait();
    }
}

fn puerto_libre() -> u16 {
    let escucha = std::net::TcpListener::bind("127.0.0.1:0").expect("sin puertos libres");
    escucha.local_addr().unwrap().port()
}

/// Una venta cualquiera de dos productos.
fn venta(n: usize) -> VentaDto {
    let skus = ["7501", "7502", "7503", "7504", "7505", "7506"];
    let sku = skus[n % skus.len()];
    VentaDto {
        lineas: vec![LineaDto {
            sku: sku.to_string(),
            descripcion: sku.to_string(),
            qty_millis: 1000 + (n as i64 % 5) * 1000,
            precio_cents: 0, // 0 = "usa el precio del catálogo"
            descuento_cents: 0,
        }],
        descuento_global_cents: 0,
        pagos: vec![PagoDto {
            metodo: "efectivo".to_string(),
            monto_cents: 0, // se rellena abajo con el total real
            recibido_cents: 0,
        }],
        cliente_id: None,
        nota: String::new(),
        supervisor: None,
    }
}

/// Cuántas ventas hay guardadas en la base (se lee con el programa caído).
fn contar_ventas(base: &Path) -> usize {
    let db = match Database::open(base) {
        Ok(d) => d,
        Err(_) => return 0,
    };
    db.conn()
        .query_row("SELECT COUNT(*) FROM sales", [], |r| r.get::<_, i64>(0))
        .map(|n| n as usize)
        .unwrap_or(0)
}

/// Revisa la base después del apagón. Devuelve la lista de problemas.
fn revisar(base: &Path) -> Vec<String> {
    let mut problemas: Vec<String> = Vec::new();
    let db = match Database::open(base) {
        Ok(d) => d,
        Err(e) => return vec![format!("la base no abrió: {e}")],
    };
    let conn = db.conn();

    match db.integrity_ok() {
        Ok(true) => {}
        Ok(false) => problemas.push("integrity_check falló".into()),
        Err(e) => problemas.push(format!("no pude verificar la integridad: {e}")),
    }

    // 3. Ninguna venta a medias: sin renglones, o con pagos que no suman.
    // Los precios del catálogo ya incluyen impuestos: el total de la venta es
    // la suma de sus renglones, y el subtotal es esa suma menos el IVA.
    let ventas: Vec<(i64, i64, i64, String)> = {
        let mut salida = Vec::new();
        if let Ok(mut stmt) =
            conn.prepare("SELECT id, total_cents, subtotal_cents, folio FROM sales")
        {
            if let Ok(mut filas) = stmt.query([]) {
                while let Ok(Some(f)) = filas.next() {
                    salida.push((
                        f.get::<_, i64>(0).unwrap_or_default(),
                        f.get::<_, i64>(1).unwrap_or_default(),
                        f.get::<_, i64>(2).unwrap_or_default(),
                        f.get::<_, String>(3).unwrap_or_default(),
                    ));
                }
            }
        }
        salida
    };

    for (id, total, subtotal_esperado, folio) in ventas {
        let renglones: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sale_items WHERE sale_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if renglones == 0 {
            problemas.push(format!("la venta {folio} quedó sin renglones"));
        }

        let pagado: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_cents), 0) FROM payments WHERE sale_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if pagado != total {
            problemas.push(format!(
                "la venta {folio} dice {total} pero sus pagos suman {pagado}"
            ));
        }

        // El total debe ser exactamente la suma de sus renglones.
        let bruto: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(unit_price_cents * qty_millis / 1000 - discount_cents), 0)
                 FROM sale_items WHERE sale_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let impuesto: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(tax_cents), 0) FROM sale_items WHERE sale_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if bruto != total {
            problemas.push(format!(
                "la venta {folio} dice {total} pero sus renglones suman {bruto}"
            ));
        }
        if bruto - impuesto != subtotal_esperado {
            problemas.push(format!(
                "la venta {folio} no cuadra el neto: {bruto} - {impuesto} != {subtotal_esperado}"
            ));
        }
    }

    // 4. El inventario cuadra con el libro de movimientos.
    let filas: Vec<(String, i64, i64)> = {
        let mut salida = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT p.sku, p.stock_millis,
                    COALESCE((SELECT SUM(qty_delta_millis) FROM inventory_movements m
                              WHERE m.product_id = p.id), 0)
             FROM products p WHERE p.track_inventory = 1",
        ) {
            if let Ok(mut f) = stmt.query([]) {
                while let Ok(Some(r)) = f.next() {
                    salida.push((
                        r.get::<_, String>(0).unwrap_or_default(),
                        r.get::<_, i64>(1).unwrap_or_default(),
                        r.get::<_, i64>(2).unwrap_or_default(),
                    ));
                }
            }
        }
        salida
    };
    for (sku, stock, movimientos) in filas {
        if stock != movimientos {
            problemas.push(format!(
                "el stock de {sku} es {stock} pero los movimientos suman {movimientos}"
            ));
        }
    }

    // 5. Nada huérfano.
    let huerfanos: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM payments p
             WHERE NOT EXISTS (SELECT 1 FROM sales s WHERE s.id = p.sale_id)",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if huerfanos > 0 {
        problemas.push(format!("{huerfanos} pagos huérfanos"));
    }
    let huerfanos_items: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sale_items si
             WHERE NOT EXISTS (SELECT 1 FROM sales s WHERE s.id = si.sale_id)",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if huerfanos_items > 0 {
        problemas.push(format!("{huerfanos_items} renglones huérfanos"));
    }

    problemas
}

#[test]
fn doscientos_apagones_no_dejan_nada_a_medias() {
    let iteraciones: usize = std::env::var("POS_ITERACIONES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);

    let raiz = std::env::temp_dir().join(format!("pos-apagon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    std::fs::create_dir_all(&raiz).unwrap();

    let mut apagones = 0usize;
    let mut ventas_intentadas = 0usize;
    let mut ventas_guardadas = 0usize;
    let mut ventas_despues_del_apagon = 0usize;
    let mut ventas_alcanzadas = 0usize;
    let mut ventas_descartadas = 0usize;
    let mut todos_los_problemas: Vec<String> = Vec::new();
    let inicio = Instant::now();

    for i in 0..iteraciones {
        let dir = raiz.join(format!("ciclo{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        let puerto = puerto_libre();
        let mut servidor = Servidor::nuevo(&dir, puerto);
        assert!(servidor.esperar(), "el servidor no arrancó en el ciclo {i}");

        let mut api = servidor.api();
        // Arranque normal del día: entrar y abrir caja.
        let token = match api.login("admin", "1234") {
            Ok(s) => s.token,
            Err(e) => {
                // Si el arranque falla aquí es un problema real, no un apagón.
                servidor.matar();
                panic!("el ciclo {i} no pudo entrar: {e}");
            }
        };
        api.ejecutar(Peticion {
            token: Some(token.clone()),
            comando: Comando::AbrirCaja {
                fondo_cents: 50_000,
            },
        });

        // Unas cuantas ventas normales...
        let normales = 2 + (i % 4);
        for n in 0..normales {
            let mut dto = venta(n);
            // El total real lo decide el catálogo; pedimos el precio y pagamos exacto.
            let productos = api
                .buscar(&token, &dto.lineas[0].sku, 1)
                .unwrap_or_default();
            if let Some(p) = productos.first() {
                dto.lineas[0].precio_cents = p.price_cents;
                let qty = dto.lineas[0].qty_millis / 1000;
                dto.pagos[0].monto_cents = p.price_cents * qty;
                dto.pagos[0].recibido_cents = p.price_cents * qty;
            } else {
                continue;
            }
            ventas_intentadas += 1;
            if api.cobrar(&token, dto).is_ok() {
                ventas_guardadas += 1;
            }
        }

        // Cuántas ventas había antes de la que vamos a interrumpir.
        let antes_de_cortar = contar_ventas(&servidor.base);

        // ...y la última la partimos a la mitad: se manda la venta y, sin
        // esperar la respuesta, se corta la luz. El tiro cae en un punto
        // distinto cada vez (0..16 ms), así que a veces alcanza a escribir el
        // cargo, a veces el movimiento de inventario, y a veces nada.
        let mut dto = venta(i);
        if let Some(p) = api
            .buscar(&token, &dto.lineas[0].sku, 1)
            .unwrap_or_default()
            .first()
        {
            dto.lineas[0].precio_cents = p.price_cents;
            let qty = dto.lineas[0].qty_millis / 1000;
            dto.pagos[0].monto_cents = p.price_cents * qty;
            dto.pagos[0].recibido_cents = p.price_cents * qty;
            ventas_intentadas += 1;

            let mut api_volando = servidor.api();
            let token_vuelo = token.clone();
            std::thread::scope(|s| {
                s.spawn(move || {
                    let _ = api_volando.ejecutar(Peticion {
                        token: Some(token_vuelo),
                        comando: Comando::Cobrar { venta: dto },
                    });
                });
                std::thread::sleep(Duration::from_millis((i as u64 * 7) % 17));
                servidor.matar();
            });
        } else {
            servidor.matar();
        }
        apagones += 1;
        drop(api);

        // --- la revisión: esto es lo que importa ---
        for p in revisar(&servidor.base) {
            todos_los_problemas.push(format!("ciclo {i}: {p}"));
        }

        // ¿La venta interrumpida se guardó completa o se fue por completo?
        // Las dos respuestas son correctas; lo que nunca puede pasar es que
        // quede a medias. Este conteo prueba que el tiro cayó en medio.
        match contar_ventas(&servidor.base).cmp(&antes_de_cortar) {
            std::cmp::Ordering::Greater => ventas_alcanzadas += 1,
            std::cmp::Ordering::Equal => ventas_descartadas += 1,
            std::cmp::Ordering::Less => {
                todos_los_problemas.push(format!("ciclo {i}: desaparecieron ventas ya guardadas"))
            }
        }

        // Y lo que sigue en el mostrador: vuelve la luz, se abre el programa
        // sobre la misma base y la tienda tiene que seguir vendiendo.
        let puerto2 = puerto_libre();
        let mut reabierto = Servidor::nuevo(&dir, puerto2);
        if reabierto.esperar() {
            let mut api2 = reabierto.api();
            if let Ok(s) = api2.login("admin", "1234") {
                api2.ejecutar(Peticion {
                    token: Some(s.token.clone()),
                    comando: Comando::AbrirCaja {
                        fondo_cents: 50_000,
                    },
                });
                let mut dto = venta(i + 1);
                if let Some(p) = api2
                    .buscar(&s.token, &dto.lineas[0].sku, 1)
                    .unwrap_or_default()
                    .first()
                {
                    dto.lineas[0].precio_cents = p.price_cents;
                    let qty = dto.lineas[0].qty_millis / 1000;
                    dto.pagos[0].monto_cents = p.price_cents * qty;
                    dto.pagos[0].recibido_cents = p.price_cents * qty;
                    ventas_intentadas += 1;
                    if api2.cobrar(&s.token, dto).is_ok() {
                        ventas_despues_del_apagon += 1;
                    }
                }
            }
            reabierto.matar();
            for p in revisar(&reabierto.base) {
                todos_los_problemas.push(format!("ciclo {i}, tras reabrir: {p}"));
            }
        } else {
            todos_los_problemas.push(format!("ciclo {i}: la base no volvió a abrir"));
        }
    }

    println!(
        "\n>> {apagones} apagones con kill -9 en {:.1?}\n>> ventas intentadas {ventas_intentadas}, guardadas {ventas_guardadas}, logradas tras reabrir {ventas_despues_del_apagon}\n>> de las ventas interrumpidas: {ventas_alcanzadas} completas, {ventas_descartadas} descartadas\n>> problemas encontrados: {}",
        inicio.elapsed(),
        todos_los_problemas.len()
    );

    assert!(
        todos_los_problemas.is_empty(),
        "la base quedó inconsistente tras los apagones:\n{}",
        todos_los_problemas.join("\n")
    );

    let _ = std::fs::remove_dir_all(&raiz);
}
