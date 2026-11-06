//! Direcciones IPv4 de esta PC, para decirle al celular a dónde apuntar.
//!
//! Sin crates extra: en Linux se pregunta a `hostname`, en Windows a
//! `ipconfig`. Si algo falla, la lista queda vacía y la interfaz lo dice
//! con palabras, no se cae.

/// IPv4 de esta máquina que no son loopback.
pub fn direcciones_lan() -> Vec<String> {
    #[cfg(windows)]
    {
        direcciones_windows()
    }
    #[cfg(not(windows))]
    {
        direcciones_unix()
    }
}

#[cfg(not(windows))]
fn direcciones_unix() -> Vec<String> {
    let Ok(salida) = std::process::Command::new("hostname").arg("-I").output() else {
        return Vec::new();
    };
    extraer_ipv4(&texto_comando(&salida.stdout))
}

#[cfg(windows)]
fn direcciones_windows() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(salida) = std::process::Command::new("ipconfig")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };
    extraer_ipv4(&texto_comando(&salida.stdout))
}

/// `ipconfig` en Windows a veces sale en UTF-16; `hostname -I` en UTF-8.
fn texto_comando(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes.iter().take(64).filter(|b| **b == 0).count() > 8 {
        let (u16s, _) = bytes.as_chunks::<2>();
        let u16s: Vec<u16> = u16s.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn extraer_ipv4(texto: &str) -> Vec<String> {
    // `ipconfig` mezcla la IP con la máscara, la puerta de enlace y los DNS.
    // Si el texto trae etiquetas, sólo se leen los renglones de dirección
    // IPv4; si no (por ejemplo `hostname -I`), se lee todo el texto.
    let con_etiquetas = texto.lines().any(|linea| es_renglon_de_ipv4(linea));
    let renglones: Vec<&str> = if con_etiquetas {
        texto
            .lines()
            .filter(|linea| es_renglon_de_ipv4(linea))
            .collect()
    } else {
        vec![texto]
    };

    let mut encontradas = Vec::new();
    for renglon in renglones {
        for token in renglon.split(|c: char| !c.is_ascii_digit() && c != '.') {
            if !es_ipv4(token) {
                continue;
            }
            if token.starts_with("127.")
                || token.starts_with("0.")
                || token == "255.255.255.255"
            {
                continue;
            }
            if !encontradas.iter().any(|e| e == token) {
                encontradas.push(token.to_string());
            }
        }
    }
    encontradas
}

fn es_renglon_de_ipv4(linea: &str) -> bool {
    let minuscula = linea.to_lowercase();
    minuscula.contains("ipv4")
        || minuscula.contains("ip address")
        || minuscula.contains("dirección ip")
}

fn es_ipv4(s: &str) -> bool {
    let partes: Vec<&str> = s.split('.').collect();
    if partes.len() != 4 {
        return false;
    }
    partes.iter().all(|p| {
        !p.is_empty()
            && p.len() <= 3
            && p.bytes().all(|b| b.is_ascii_digit())
            && p.parse::<u16>().ok().is_some_and(|n| n <= 255)
    })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn extrae_las_ipv4_y_ignora_loopback() {
        let t = "eth0: 192.168.1.50  127.0.0.1  10.0.0.8\n";
        assert_eq!(extraer_ipv4(t), vec!["192.168.1.50", "10.0.0.8"]);
    }

    #[test]
    fn entiende_el_ipconfig_en_espanol() {
        let t = "Dirección IPv4. . . . . . . . . . . . : 192.168.0.21\r\n";
        assert_eq!(extraer_ipv4(t), vec!["192.168.0.21"]);
    }

    #[test]
    fn no_confunde_la_mascara_ni_la_puerta_con_la_ip() {
        let t = "Adaptador Ethernet:\r\n\
                 Dirección IPv4. . . . . . . . . . . . : 192.168.1.50\r\n\
                 Máscara de subred . . . . . . . . . . : 255.255.255.0\r\n\
                 Puerta de enlace predeterminada . . . : 192.168.1.1\r\n\
                 Servidores DNS. . . . . . . . . . . . : 8.8.8.8\r\n";
        assert_eq!(extraer_ipv4(t), vec!["192.168.1.50"]);
    }

    #[test]
    fn no_inventa_si_no_hay_nada() {
        assert!(extraer_ipv4("hostname: invalid option -- I").is_empty());
    }
}
