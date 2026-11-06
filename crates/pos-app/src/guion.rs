//! Ejecutor de guiones headless.
//!
//! Un guion es un JSON con una lista de comandos de [`pos_api::Comando`]. Se
//! ejecutan contra la misma `LocalApi` que usa la interfaz, sin abrir ventana,
//! y el resultado se puede guardar como reporte. Sirve para que una persona
//! (o un modelo) compruebe flujos completos sin pelearse con clics ni con la
//! geometría de la pantalla.
//!
//! Formato mínimo:
//!
//! ```json
//! {
//!   "nombre": "humo de venta",
//!   "reporte": "reportes/humo.json",
//!   "pasos": [
//!     { "descripcion": "entrar", "comando": { "Login": { "username": "admin", "pin": "1234" } } },
//!     { "descripcion": "buscar", "comando": { "BuscarProductos": { "termino": "", "limite": 5 } } }
//!   ]
//! }
//! ```
//!
//! Cada paso acepta `esperar`: `"ok"` (por omisión), `"error"` o
//! `"cualquiera"`. El token de la sesión se guarda solo después de un `Login`
//! y se manda en los pasos siguientes. Con `"json": true` se imprime el
//! reporte completo en vez de las líneas legibles.

use std::path::Path;
use std::time::Instant;

use pos_api::{Comando, Cuerpo, Peticion, PosApi, Respuesta};
use serde_json::{json, Value};

/// Ejecuta el guion y devuelve el código de salida (0 = todo bien).
pub fn ejecutar(api: Box<dyn PosApi>, ruta: &str, db_path: &str) -> i32 {
    let texto = match std::fs::read_to_string(ruta) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("No se pudo leer el guion '{ruta}': {e}");
            return 2;
        }
    };
    let guion: Value = match serde_json::from_str(&texto) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("El guion '{ruta}' no es JSON válido: {e}");
            return 2;
        }
    };
    let pasos = match guion.get("pasos").and_then(Value::as_array) {
        Some(p) if !p.is_empty() => p,
        _ => {
            eprintln!("El guion '{ruta}' necesita un arreglo 'pasos' con al menos un paso.");
            return 2;
        }
    };

    let nombre = guion
        .get("nombre")
        .and_then(Value::as_str)
        .unwrap_or("guion");
    let detener = guion
        .get("detener_al_fallar")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let solo_json = guion.get("json").and_then(Value::as_bool).unwrap_or(false);
    let ruta_reporte = guion.get("reporte").and_then(Value::as_str);

    let mut api = api;
    let mut token: Option<String> = None;
    let mut resultados: Vec<Value> = Vec::new();
    let mut fallos = 0usize;
    let inicio = Instant::now();

    if !solo_json {
        println!("Guion: {nombre}");
        println!("Base:  {db_path}");
        println!("Pasos: {}", pasos.len());
        println!();
    }

    for (indice, paso) in pasos.iter().enumerate() {
        let numero = indice + 1;
        let descripcion = paso
            .get("descripcion")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("paso {numero}"));
        let esperado = paso
            .get("esperar")
            .and_then(Value::as_str)
            .unwrap_or("ok");
        let esperado_valido = matches!(esperado, "ok" | "error" | "cualquiera");

        let comando: Comando = match paso.get("comando") {
            Some(valor) => match serde_json::from_value(valor.clone()) {
                Ok(c) => c,
                Err(e) => {
                    fallos += 1;
                    if !solo_json {
                        println!("[{numero}/{}] {descripcion} ... FALLO", pasos.len());
                        println!("       comando inválido: {e}");
                    }
                    resultados.push(json!({
                        "paso": numero,
                        "descripcion": descripcion,
                        "esperado": esperado,
                        "ok": false,
                        "duracion_ms": 0,
                        "error": format!("comando inválido: {e}"),
                        "respuesta": Value::Null,
                    }));
                    if detener {
                        break;
                    }
                    continue;
                }
            },
            None => {
                fallos += 1;
                if !solo_json {
                    println!("[{numero}/{}] {descripcion} ... FALLO", pasos.len());
                    println!("       falta el campo 'comando'");
                }
                resultados.push(json!({
                    "paso": numero,
                    "descripcion": descripcion,
                    "esperado": esperado,
                    "ok": false,
                    "duracion_ms": 0,
                    "error": "falta el campo 'comando'",
                    "respuesta": Value::Null,
                }));
                if detener {
                    break;
                }
                continue;
            }
        };

        let salir = matches!(&comando, Comando::Salir);
        let t0 = Instant::now();
        let respuesta = api.ejecutar(Peticion {
            token: token.clone(),
            comando,
        });
        let duracion_ms = t0.elapsed().as_millis();

        if let Respuesta::Ok(Cuerpo::Sesion(sesion)) = &respuesta {
            token = Some(sesion.token.clone());
        }
        if salir {
            token = None;
        }

        let cumple = if !esperado_valido {
            false
        } else {
            match esperado {
                "ok" => matches!(&respuesta, Respuesta::Ok(_)),
                "error" => matches!(&respuesta, Respuesta::Error(_)),
                _ => true,
            }
        };
        if !cumple {
            fallos += 1;
        }

        let resumen = resumen_respuesta(&respuesta);
        if !solo_json {
            let marca = if cumple { "OK" } else { "FALLO" };
            println!("[{numero}/{}] {descripcion} ... {marca} ({duracion_ms} ms)", pasos.len());
            println!("       {resumen}");
            if !esperado_valido {
                println!("       'esperar' inválido: {esperado}");
            }
        }

        resultados.push(json!({
            "paso": numero,
            "descripcion": descripcion,
            "esperado": esperado,
            "ok": cumple,
            "duracion_ms": duracion_ms,
            "respuesta": serde_json::to_value(&respuesta).unwrap_or(Value::Null),
        }));

        if !cumple && detener {
            break;
        }
    }

    let ejecutados = resultados.len();
    let reporte = json!({
        "nombre": nombre,
        "base": db_path,
        "total": pasos.len(),
        "ejecutados": ejecutados,
        "ok": ejecutados.saturating_sub(fallos),
        "fallos": fallos,
        "duracion_ms": inicio.elapsed().as_millis(),
        "resultados": resultados,
    });

    if let Some(ruta) = ruta_reporte {
        if let Some(dir) = Path::new(ruta).parent() {
            if !dir.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(dir);
            }
        }
        match std::fs::write(ruta, serde_json::to_string_pretty(&reporte).unwrap_or_default()) {
            Ok(()) => {
                if !solo_json {
                    println!("\nReporte: {ruta}");
                }
            }
            Err(e) => eprintln!("No se pudo escribir el reporte '{ruta}': {e}"),
        }
    }

    if solo_json {
        println!("{}", serde_json::to_string_pretty(&reporte).unwrap_or_default());
    } else {
        println!(
            "\nResumen: {}/{} OK, {} fallo(s), {} ms",
            ejecutados.saturating_sub(fallos),
            ejecutados,
            fallos,
            reporte["duracion_ms"].as_u64().unwrap_or(0)
        );
    }

    if fallos == 0 {
        0
    } else {
        1
    }
}

fn resumen_respuesta(respuesta: &Respuesta) -> String {
    match respuesta {
        Respuesta::Error(e) => format!("error: {e}"),
        Respuesta::Ok(cuerpo) => match cuerpo {
            Cuerpo::Vacio => "ok".to_string(),
            Cuerpo::Sesion(s) => format!("sesión de {}", s.usuario.display_name),
            Cuerpo::Productos(p) => format!("{} producto(s)", p.len()),
            Cuerpo::Caja(c) => {
                if c.abierta {
                    format!(
                        "caja abierta · fondo {} · {} ticket(s)",
                        c.fondo_cents, c.tickets
                    )
                } else {
                    "caja cerrada".to_string()
                }
            }
            Cuerpo::Venta(v) => format!(
                "folio {} · total {} centavos · cambio {} centavos",
                v.folio, v.total_cents, v.cambio_cents
            ),
            Cuerpo::Texto(t) => t.clone(),
            otro => {
                let texto = serde_json::to_string(otro).unwrap_or_default();
                if texto.chars().count() > 160 {
                    let corto: String = texto.chars().take(157).collect();
                    format!("{corto}...")
                } else {
                    texto
                }
            }
        },
    }
}
