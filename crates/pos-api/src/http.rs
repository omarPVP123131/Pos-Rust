//! Cliente HTTP: **el mismo trait `PosApi`**, pero por la red.
//!
//! Es la prueba de que la API está bien desacoplada: cambiar `LocalApi` por
//! `HttpApi` no cambia ni una línea de la lógica de negocio. La app móvil, otra
//! caja o una terminal de inventario usan exactamente esto.
//!
//! Sin `reqwest` ni `tokio`: HTTP/1.1 sobre `std::net::TcpStream`, que es todo
//! lo que se necesita para hablar con el servidor de la LAN.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::api::{Peticion, PosApi, Respuesta};

pub struct HttpApi {
    /// Por ejemplo `http://192.168.1.50:8080`
    base: String,
    timeout: Duration,
}

impl HttpApi {
    pub fn new(base: impl Into<String>) -> Self {
        HttpApi {
            base: base.into().trim_end_matches('/').to_string(),
            timeout: Duration::from_secs(5),
        }
    }

    pub fn con_timeout(base: impl Into<String>, timeout: Duration) -> Self {
        let mut a = Self::new(base);
        a.timeout = timeout;
        a
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// `POST {base}/api` con el JSON de la petición.
    pub fn post(&self, peticion: &Peticion) -> Result<Respuesta, String> {
        let cuerpo =
            serde_json::to_string(peticion).map_err(|e| format!("No se pudo serializar: {e}"))?;

        let (host, autoridad, ruta) = partir_url(&format!("{}/api", self.base))?;
        let puerto = puerto_de(&host);

        let mut stream = TcpStream::connect((autoridad.as_str(), puerto))
            .map_err(|e| format!("No se pudo conectar a {host}: {e}"))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|e| e.to_string())?;

        let peticion_http = format!(
            "POST {ruta} HTTP/1.1\r\n\
             Host: {host}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n\
             {cuerpo}",
            cuerpo.len()
        );
        stream
            .write_all(peticion_http.as_bytes())
            .map_err(|e| format!("No se pudo enviar: {e}"))?;

        // --- leer encabezados ---
        let mut lector = BufReader::new(stream);
        let mut linea = String::new();
        lector
            .read_line(&mut linea)
            .map_err(|e| format!("Sin respuesta: {e}"))?;
        let codigo = linea.split_whitespace().nth(1).unwrap_or("500").to_string();
        let mut longitud: Option<usize> = None;
        loop {
            let mut cabecera = String::new();
            if lector.read_line(&mut cabecera).is_err() || cabecera.trim().is_empty() {
                break;
            }
            let c = cabecera.to_lowercase();
            if let Some(valor) = c.strip_prefix("content-length:") {
                longitud = valor.trim().parse().ok();
            }
        }

        let mut cuerpo_bytes = Vec::new();
        if let Some(n) = longitud {
            let mut trozo = lector.take(n as u64);
            trozo
                .read_to_end(&mut cuerpo_bytes)
                .map_err(|e| e.to_string())?;
        } else {
            lector
                .read_to_end(&mut cuerpo_bytes)
                .map_err(|e| e.to_string())?;
        }

        if !codigo.starts_with('2') {
            return Err(format!("El servidor respondió {codigo}"));
        }
        serde_json::from_slice(&cuerpo_bytes)
            .map_err(|e| format!("Respuesta ilegible del servidor: {e}"))
    }
}

impl PosApi for HttpApi {
    fn ejecutar(&mut self, peticion: Peticion) -> Respuesta {
        match self.post(&peticion) {
            Ok(r) => r,
            Err(e) => Respuesta::Error(e),
        }
    }
}

/// Separa `http://host:puerto/ruta` en sus partes.
fn partir_url(url: &str) -> Result<(String, String, String), String> {
    let sin_esquema = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let (autoridad, ruta) = match sin_esquema.find('/') {
        Some(i) => (&sin_esquema[..i], sin_esquema[i..].to_string()),
        None => (sin_esquema, "/".to_string()),
    };
    // La autoridad puede incluir puerto; para conectar hay que separarlo.
    let host_sin_puerto = autoridad.split(':').next().unwrap_or(autoridad).to_string();
    Ok((autoridad.to_string(), host_sin_puerto, ruta))
}

fn puerto_de(autoridad: &str) -> u16 {
    autoridad
        .split(':')
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parte_la_url_correctamente() {
        let (host, autoridad, ruta) = partir_url("http://192.168.1.50:8080/api").unwrap();
        assert_eq!(host, "192.168.1.50:8080");
        assert_eq!(autoridad, "192.168.1.50");
        assert_eq!(ruta, "/api");
        assert_eq!(puerto_de(&host), 8080);

        let (host, _, ruta) = partir_url("http://localhost/").unwrap();
        assert_eq!(ruta, "/");
        assert_eq!(puerto_de(&host), 80);
    }

    #[test]
    fn una_api_inaccesible_devuelve_error_no_panico() {
        // Puerto cerrado: debe fallar limpio (lo usa la app móvil sin señal).
        let mut api = HttpApi::con_timeout("http://127.0.0.1:1", Duration::from_millis(300));
        match api.ejecutar(Peticion {
            token: None,
            comando: crate::Comando::Usuarios,
        }) {
            Respuesta::Error(_) => {}
            otro => panic!("se esperaba error, llegó {otro:?}"),
        }
    }

    #[test]
    fn el_json_es_el_que_entiende_el_celular() {
        // Contrato del script `scripts/cliente_movil.py` y de cualquier
        // app tonta: POST /api con {token, comando} en JSON etiquetado.
        let p = Peticion {
            token: None,
            comando: crate::Comando::UsuariosLogin,
        };
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"token":null,"comando":"UsuariosLogin"}"#
        );
        let p = Peticion {
            token: Some("abc".into()),
            comando: crate::Comando::Login {
                username: "admin".into(),
                pin: "1234".into(),
            },
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains(r#""token":"abc""#), "{s}");
        assert!(s.contains(r#""Login""#), "{s}");
        assert!(s.contains(r#""username":"admin""#), "{s}");
    }
}
