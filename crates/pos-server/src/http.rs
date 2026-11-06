//! Servidor HTTP de la LAN, escrito con `std::net` nada más.
//!
//! Un hilo por conexión. Suena ingenuo, pero en un mostrador hay a lo sumo
//! tres dispositivos hablando y el servidor vive en la misma PC que ya está
//! corriendo el punto de venta: aquí cada mega cuenta más que la escala.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use pos_api::{Peticion, PosApi, Respuesta};

/// Instancia compartida del api (la de escritorio o una local, da igual).
pub type ApiCompartida = Arc<Mutex<Box<dyn PosApi>>>;

/// Deja escuchando el servidor. **Bloquea** el hilo actual: lanzalo en un
/// hilo aparte o como modo exclusivo del ejecutable.
pub fn servir(api: ApiCompartida, direccion: &str) -> std::io::Result<()> {
    let escucha = TcpListener::bind(direccion)?;
    println!(
        "[pos-server] escuchando en http://{}",
        escucha.local_addr()?
    );
    for conexion in escucha.incoming() {
        match conexion {
            Ok(stream) => {
                let api = Arc::clone(&api);
                if let Err(e) = thread::Builder::new()
                    .name("pos-conexion".into())
                    .spawn(move || atender(stream, api))
                {
                    eprintln!("[pos-server] no se pudo atender la conexión: {e}");
                }
            }
            Err(e) => eprintln!("[pos-server] conexión rechazada: {e}"),
        }
    }
    Ok(())
}

/// Igual que [`servir`] pero escoge un puerto libre: para pruebas y para no
/// pelearse con el puerto ocupado.
pub fn servir_en_puerto_libre(api: ApiCompartida, host: &str) -> std::io::Result<u16> {
    let escucha = TcpListener::bind(format!("{host}:0"))?;
    let puerto = escucha.local_addr()?.port();
    thread::Builder::new()
        .name("pos-servidor".into())
        .spawn(move || {
            for conexion in escucha.incoming().flatten() {
                let api = Arc::clone(&api);
                let _ = thread::Builder::new()
                    .name("pos-conexion".into())
                    .spawn(move || atender(conexion, api));
            }
        })?;
    Ok(puerto)
}

fn atender(stream: TcpStream, api: ApiCompartida) {
    if let Err(e) = manejar(stream, api) {
        eprintln!("[pos-server] {e}");
    }
}

fn manejar(stream: TcpStream, api: ApiCompartida) -> std::io::Result<()> {
    let mut lector = BufReader::new(stream.try_clone()?);

    // --- línea de petición ---
    let mut primera = String::new();
    if lector.read_line(&mut primera)? == 0 {
        return Ok(());
    }
    let partes: Vec<&str> = primera.split_whitespace().collect();
    let (metodo, ruta) = (
        partes.first().copied().unwrap_or("GET").to_string(),
        partes.get(1).copied().unwrap_or("/").to_string(),
    );

    // --- encabezados ---
    let mut longitud = 0usize;
    loop {
        let mut linea = String::new();
        if lector.read_line(&mut linea)? == 0 || linea.trim().is_empty() {
            break;
        }
        let minuscula = linea.to_lowercase();
        if let Some(valor) = minuscula.strip_prefix("content-length:") {
            longitud = valor.trim().parse().unwrap_or(0);
        }
    }

    // --- cuerpo ---
    let mut cuerpo = vec![0u8; longitud];
    if longitud > 0 {
        use std::io::Read;
        lector.read_exact(&mut cuerpo)?;
    }

    let respuesta = if metodo == "OPTIONS" {
        // Preflight del navegador / WebView de la app móvil.
        String::new()
    } else if ruta == "/api/salud" {
        r#"{"estado":"ok"}"#.to_string()
    } else if ruta == "/api" && metodo == "POST" {
        let respuesta_api = match serde_json::from_slice::<Peticion>(&cuerpo) {
            Ok(peticion) => {
                let mut guarda = match api.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        eprintln!("[pos-server] api envenenada: {e}");
                        return Ok(());
                    }
                };
                guarda.ejecutar(peticion)
            }
            Err(e) => Respuesta::Error(format!("Petición inválida: {e}")),
        };
        serde_json::to_string(&respuesta_api).unwrap_or_else(|e| {
            serde_json::to_string(&Respuesta::Error(e.to_string())).unwrap_or_default()
        })
    } else {
        serde_json::to_string(&Respuesta::Error(
            "Usa POST /api con un comando del punto de venta".to_string(),
        ))
        .unwrap_or_default()
    };

    let cabeceras = formato_respuesta(&respuesta);
    let mut stream = stream;
    stream.write_all(cabeceras.as_bytes())?;
    stream.write_all(respuesta.as_bytes())?;
    stream.flush()
}

/// HTTP/1.1 con CORS: la app móvil (WebView o HTTP nativo) necesita permiso
/// explícito para llamar desde otro origen.
fn formato_respuesta(cuerpo: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: POST, GET, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Connection: close\r\n\r\n",
        cuerpo.len()
    )
}

#[cfg(test)]
mod tests {
    /// Prueba de humo: levanta el servidor en un puerto libre y lo apaga.
    #[test]
    fn arranca_y_responde_salud() {
        use super::*;
        use pos_api::LocalApi;
        use pos_db::Database;

        let db = Database::open_in_memory().unwrap();
        let api: ApiCompartida = Arc::new(Mutex::new(Box::new(LocalApi::new(db))));
        let puerto = servir_en_puerto_libre(api, "127.0.0.1").unwrap();

        // HTTP a mano, sin cliente: así comprobamos el servidor mismo.
        let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{puerto}")).unwrap();
        stream
            .write_all(b"GET /api/salud HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let mut respuesta = String::new();
        use std::io::Read;
        stream.read_to_string(&mut respuesta).unwrap();
        assert!(respuesta.contains("200 OK"));
        assert!(respuesta.contains("\"estado\":\"ok\""));
    }
}
