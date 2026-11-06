//! Herramienta para firmar actualizaciones. **No va instalada en la tienda.**
//!
//! ```bash
//! # una sola vez, en la máquina donde se firma:
//! cargo run -p pos-update --bin pos-firmar --features firmar -- --generar
//!
//! # cada vez que se suelta una versión:
//! cargo run -p pos-update --bin pos-firmar -- --firmar target/release/pos \
//!     --version 0.5.0 --notas "Arregla el corte de caja" --salida 0.5.0.posup
//! ```
//!
//! La llave privada se guarda en `llaves/pos-privada.txt`. **No se sube al
//! repositorio y no se manda a nadie**: quien la tenga puede firmar cualquier
//! cosa. La pública sí va dentro del binario (`llaves/pos-publica.txt`).

use std::path::{Path, PathBuf};

use pos_update::paquete::armar;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let valor = |bandera: &str| -> Option<String> {
        args.iter()
            .position(|a| a == bandera)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let tiene = |bandera: &str| args.iter().any(|a| a == bandera);

    if tiene("--ayuda") || tiene("-h") || args.len() == 1 {
        ayuda();
        return;
    }

    if tiene("--generar") {
        generar(valor("--salida").unwrap_or_else(|| "llaves".to_string()));
        return;
    }

    let Some(ejecutable) = valor("--firmar") else {
        eprintln!("Falta qué firmar: pos-firmar --firmar RUTA --version X.Y.Z");
        std::process::exit(2);
    };
    let Some(version) = valor("--version") else {
        eprintln!("Falta la versión: --version X.Y.Z");
        std::process::exit(2);
    };
    let salida = valor("--salida").unwrap_or_else(|| format!("punto-de-venta-{version}.posup"));
    let min_version = valor("--min").unwrap_or_else(|| "0.0.0".to_string());
    let notas = valor("--notas").unwrap_or_default();

    let privada = match leer_privada(valor("--llave")) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let bytes = match std::fs::read(&ejecutable) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("No pude leer {ejecutable}: {e}");
            std::process::exit(1);
        }
    };

    match armar(&version, &min_version, &notas, &bytes, &privada) {
        Ok(paquete) => {
            if let Err(e) = std::fs::write(&salida, &paquete) {
                eprintln!("No pude escribir {salida}: {e}");
                std::process::exit(1);
            }
            let publica = pos_update::a_hex(
                &ed25519_dalek::SigningKey::from_bytes(&privada)
                    .verifying_key()
                    .to_bytes(),
            );
            println!("Firmado: {salida}");
            println!("  versión      {version} (desde la {min_version})");
            println!("  ejecutable   {} bytes", bytes.len());
            println!("  paquete      {} bytes", paquete.len());
            println!("  huella       {}", pos_update::aplicar::huella(&bytes));
            println!("  llave pública {publica}");
            println!("\nÉsta es la llave con la que se firmó: si no es la que trae el\nprograma compilado, la tienda lo va a rechazar.");
        }
        Err(e) => {
            eprintln!("No pude armar el paquete: {e}");
            std::process::exit(1);
        }
    }
}

/// Crea un par de llaves nuevo. Sólo con `--features firmar`: así la
/// herramienta no le mete un generador de aleatorios al binario de la tienda.
#[cfg(feature = "firmar")]
fn generar(dir: String) {
    use rand_core::RngCore;
    use std::path::PathBuf;
    let mut semilla = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut semilla);
    let privada = ed25519_dalek::SigningKey::from_bytes(&semilla);
    let publica = privada.verifying_key().to_bytes();
    let dir = PathBuf::from(dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("No pude crear {}: {e}", dir.display());
        std::process::exit(1);
    }
    let p_priv = dir.join("pos-privada.txt");
    let p_pub = dir.join("pos-publica.txt");
    if let Err(e) = std::fs::write(&p_priv, format!("{}\n", pos_update::a_hex(&semilla))) {
        eprintln!("No pude escribir {}: {e}", p_priv.display());
        std::process::exit(1);
    }
    if let Err(e) = std::fs::write(&p_pub, format!("{}\n", pos_update::a_hex(&publica))) {
        eprintln!("No pude escribir {}: {e}", p_pub.display());
        std::process::exit(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p_priv, std::fs::Permissions::from_mode(0o600));
    }
    println!("Llaves nuevas en {}", dir.display());
    println!(
        "  pública  {}  (va dentro del programa: se recompila)",
        p_pub.display()
    );
    println!(
        "  privada  {}  (NO se comparte con nadie)",
        p_priv.display()
    );
}

#[cfg(not(feature = "firmar"))]
fn generar(_dir: String) {
    eprintln!(
        "Para generar llaves hay que compilar con la característica 'firmar':\n  \
         cargo run -p pos-update --bin pos-firmar --features firmar -- --generar"
    );
    std::process::exit(2);
}

/// Lee la llave privada: de un archivo, de la bandera o de `llaves/`.
fn leer_privada(ruta: Option<String>) -> Result<[u8; 32], String> {
    let texto = match ruta {
        Some(r) => {
            std::fs::read_to_string(&r).map_err(|e| format!("No pude leer la llave {r}: {e}"))?
        }
        None => {
            // Primero junto al crate (como queda al compilar el workspace) y
            // luego en el directorio actual.
            let candidatos = [
                Path::new(env!("CARGO_MANIFEST_DIR")).join("llaves/pos-privada.txt"),
                PathBuf::from("llaves/pos-privada.txt"),
                PathBuf::from("pos-privada.txt"),
            ];
            let encontrada = candidatos.iter().find(|c| c.exists());
            match encontrada {
                Some(r) => std::fs::read_to_string(r).map_err(|e| e.to_string())?,
                None => {
                    return Err("No encontré la llave privada. Pásala con --llave RUTA, \
                         déjala en llaves/pos-privada.txt o genera una con --generar"
                        .into())
                }
            }
        }
    };
    let bytes = pos_update::de_hex(texto.trim()).ok_or("La llave privada no es hexadecimal")?;
    bytes
        .try_into()
        .map_err(|_| "La llave privada debe traer 32 bytes (64 caracteres hex)".to_string())
}

fn ayuda() {
    println!(
        "pos-firmar — firma paquetes de actualización del punto de venta\n\n\
         Uso:\n  \
         pos-firmar --generar [--salida DIR]\n  \
         pos-firmar --firmar EJECUTABLE --version X.Y.Z [opciones]\n\n\
         Opciones:\n  \
         --min VERSION    versión mínima desde la que se puede subir\n  \
         --notas TEXTO    lo que verá el encargado antes de aceptar\n  \
         --salida ARCHIVO dónde dejar el .posup\n  \
         --llave RUTA     llave privada (por defecto llaves/pos-privada.txt)\n"
    );
}
