//! En Windows mete el icono y el manifiesto DPI dentro del .exe.
//!
//! Sin esto, el programa se ve como un recuadro genérico en la barra de
//! tareas y Windows lo escala borroso en pantallas de alta densidad. El
//! instalador ya pone el .ico en los atajos; esto es para el ejecutable mismo.
//!
//! Se corre también al cruzar desde Linux (`x86_64-w64-mingw32-windres`).
//! Si no hay windres, el .exe igual funciona: sólo le faltan icono y DPI.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/pos.rc");
    println!("cargo:rerun-if-changed=../../packaging/windows/app.manifest");
    println!("cargo:rerun-if-changed=../../packaging/iconos/punto-de-venta.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let raiz = manifest.join("../..");
    let win = raiz.join("packaging/windows");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("pos_icon.res");

    let windres = std::env::var("WINDRES").unwrap_or_else(|_| {
        if cfg!(unix) {
            "x86_64-w64-mingw32-windres".into()
        } else {
            "windres".into()
        }
    });

    let salida = std::process::Command::new(&windres)
        .current_dir(&win)
        .args(["pos.rc", "-O", "coff", "-o"])
        .arg(&out)
        .output();

    match salida {
        Ok(s) if s.status.success() && out.exists() => {
            println!("cargo:rustc-link-arg={}", out.display());
        }
        Ok(s) => {
            let err = String::from_utf8_lossy(&s.stderr);
            println!("cargo:warning=no pude embeber el icono ({windres}): {err}");
        }
        Err(e) => {
            println!("cargo:warning=no pude embeber el icono ({windres}): {e}");
        }
    }
}
