//! Descubrir qué impresora hay conectada, sin adivinar.
//!
//! La versión anterior de este archivo era un arreglo de cinco rutas fijas y
//! preguntaba `¿existe?`. Eso falla de dos maneras: se equivoca con la que sí
//! existe (un adaptador serie que no es la impresora) y no dice nada útil
//! cuando no hay nada.
//!
//! Aquí la detección mira de verdad:
//!   * recorre `/dev/usb/lp*`, `/dev/ttyUSB*`, `/dev/ttyACM*`, `/dev/ttyS*`;
//!   * lee el VID/PID del dispositivo en `/sys` (archivo `uevent`);
//!   * con eso dice el fabricante, el transporte y el perfil sugerido.
//!
//! Y es probable: se puede correr contra un árbol falso
//! ([`enumerar_en`]), que es como se prueba en la máquina de desarrollo sin
//! tener una impresora enfrente.

use std::path::{Path, PathBuf};

use crate::perfil::Perfil;
use crate::transporte::Transporte;

/// Una impresora (o un puerto que podría serlo) encontrada en el equipo.
#[derive(Clone, Debug, PartialEq)]
pub struct ImpresoraDetectada {
    /// Ruta del dispositivo: `/dev/usb/lp0`, `/dev/ttyUSB0`.
    pub ruta: String,
    /// Cómo hay que hablarle.
    pub transporte: Transporte,
    /// Nombre para el encargado: «Epson (USB)».
    pub etiqueta: String,
    /// Fabricante si se reconoció; si no, algo honesto como «Desconocido».
    pub fabricante: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    /// ¿Hay indicios reales de que es una impresora (clase USB 07)?
    pub es_impresora: bool,
    /// Perfil con el que conviene arrancar.
    pub perfil: Perfil,
    /// Nota para la pantalla: qué se supo y qué no.
    pub nota: String,
}

/// Busca impresoras en el equipo.
pub fn enumerar() -> Vec<ImpresoraDetectada> {
    #[cfg(windows)]
    {
        enumerar_windows()
    }
    #[cfg(not(windows))]
    {
        enumerar_en(Path::new("/dev"), Path::new("/sys"))
    }
}

/// La mejor candidata, sin adivinar.
///
/// Sólo se propone una impresora **confirmada** (clase USB 07, o una
/// instalada en Windows). Un adaptador serie o un `/dev/ttyS0` de la placa
/// no se eligen solos: el encargado los prueba.
pub fn mejor(lista: &[ImpresoraDetectada]) -> Option<&ImpresoraDetectada> {
    lista.iter().find(|i| i.es_impresora)
}

/// Colas de Windows que no son una térmica: PDF, Fax, XPS, OneNote.
///
/// El spooler las lista igual que una POS-80. Si se toman como impresora de
/// tickets, Detectar "adivinaría" y el ticket se iría a un archivo. No.
pub fn es_cola_virtual(nombre: &str) -> bool {
    let n = nombre.to_ascii_lowercase();
    n.contains("pdf")
        || n.contains("xps")
        || n.contains("fax")
        || n.contains("onenote")
        || n.contains("microsoft print to")
        || n.contains("document writer")
}

/// Orden al listar en Windows: primero las que parecen térmica, al final PDF/Fax.
///
/// No adivina el modelo: sólo ordena lo que Windows ya tiene instalado.
pub fn puntuacion_termica(nombre: &str) -> u8 {
    if es_cola_virtual(nombre) {
        return 2;
    }
    let n = nombre.to_ascii_lowercase();
    if n.contains("pos")
        || n.contains("tm-")
        || n.contains("tm ")
        || n.contains("epson")
        || n.contains("star")
        || n.contains("bixolon")
        || n.contains("citizen")
        || n.contains("thermal")
        || n.contains("ticket")
        || n.contains("80mm")
        || n.contains("58mm")
    {
        0
    } else {
        1
    }
}

/// Traduce el código de Windows al consejo del mostrador.
pub fn consejo_error_spooler(codigo: u32, nombre: &str) -> String {
    match codigo {
        5 => format!(
            "Windows no dejó abrir «{nombre}». ¿Está pausada o la está usando otro programa?"
        ),
        1722 => {
            "El spooler de Windows no responde. Abre Servicios y arranca «Cola de impresión»."
                .into()
        }
        1801 | 1902 => format!(
            "No hay una impresora que se llame «{nombre}». Pulsa Detectar o revisa Impresoras y escáneres."
        ),
        _ => format!(
            "no se pudo abrir «{nombre}» (Windows {codigo}). ¿Sigue instalada en Impresoras y escáneres? El programa imprime en modo RAW."
        ),
    }
}

/// Enumeración sobre un árbol cualquiera. Existe para poder probarla.
pub fn enumerar_en(dev: &Path, sys: &Path) -> Vec<ImpresoraDetectada> {
    let mut encontradas: Vec<ImpresoraDetectada> = Vec::new();

    // 1. Impresoras USB de clase impresora: /dev/usb/lpN (y /dev/usblpN).
    for ruta in glob(dev, &[("usb", "lp"), ("", "usblp")]) {
        if let Some(i) = describir(&ruta, sys) {
            encontradas.push(i);
        }
    }

    // 2. Puertos serie: adaptadores USB-serial y puertos de la placa.
    for ruta in glob(
        dev,
        &[("", "ttyUSB"), ("", "ttyACM"), ("", "ttyS"), ("", "ttyAMA")],
    ) {
        if let Some(i) = describir(&ruta, sys) {
            encontradas.push(i);
        }
    }

    // 3. Los enlaces /dev/serial/by-id no aportan dispositivos nuevos, pero sí
    //    un nombre legible: se usa para bautizar al que ya encontramos.
    for enlace in glob(dev, &[("serial/by-id", "")]) {
        let destino = std::fs::canonicalize(&enlace).ok();
        let destino = destino
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if let Some(i) = encontradas
            .iter_mut()
            .find(|i| !destino.is_empty() && destino == i.ruta)
        {
            if let Some(nombre) = nombre_del_enlace(&enlace) {
                if i.fabricante == "Desconocido" {
                    i.fabricante = nombre.clone();
                }
                i.etiqueta = format!("{nombre} ({})", i.etiqueta);
            }
        }
    }

    encontradas.sort_by(|a, b| {
        b.es_impresora
            .cmp(&a.es_impresora)
            .then_with(|| orden_del_puerto(&a.ruta).cmp(&orden_del_puerto(&b.ruta)))
            .then_with(|| a.ruta.cmp(&b.ruta))
    });
    encontradas
}

/// Orden en que conviene proponer los puertos.
///
/// Importa más de lo que parece: casi cualquier PC de escritorio tiene un
/// `/dev/ttyS0` en la placa que no está conectado a nada. Si se propone antes
/// que el adaptador USB de la impresora, el encargado configura el puerto
/// equivocado y el ticket no sale nunca.
fn orden_del_puerto(ruta: &str) -> u8 {
    // En Windows la ruta viene con `\` (C:\...\ttyUSB0): se parte por las
    // dos barras, si no todo es "puerto de placa" y el USB queda al final.
    let nombre = ruta.rsplit(['/', '\\']).next().unwrap_or(ruta);
    if nombre.starts_with("lp") || nombre.starts_with("usblp") {
        0 // impresora USB
    } else if nombre.starts_with("ttyUSB") || nombre.starts_with("ttyACM") {
        1 // adaptador USB-serial: lo que usan las chinas
    } else {
        2 // puerto de la placa (ttyS0...): casi siempre vacío
    }
}

/// Lista los archivos de `dev` que empiezan por alguno de los prefijos.
/// `("", "ttyUSB")` = `/dev/ttyUSB*`; `("usb", "lp")` = `/dev/usb/lp*`.
fn glob(dev: &Path, prefijos: &[(&str, &str)]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (carpeta, prefijo) in prefijos {
        let dir = if carpeta.is_empty() {
            dev.to_path_buf()
        } else {
            dev.join(carpeta)
        };
        let entradas = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entrada in entradas.flatten() {
            let nombre = entrada.file_name();
            let nombre = nombre.to_string_lossy().to_string();
            if nombre.starts_with(prefijo) {
                out.push(entrada.path());
            }
        }
    }
    out.sort();
    out
}

/// Lee el nombre bonito de un enlace `/dev/serial/by-id/usb-1a86_USB2.0-Serial...`
fn nombre_del_enlace(enlace: &Path) -> Option<String> {
    let nombre = enlace.file_name()?.to_string_lossy().to_string();
    let limpio = nombre.trim_start_matches("usb-");
    let partes: Vec<&str> = limpio.split('_').collect();
    // "1a86_USB2.0-Serial" -> se toma lo que sigue al identificador.
    let etiqueta = match partes.len() {
        0 | 1 => limpio.to_string(),
        _ => partes[1..].join(" "),
    };
    let etiqueta = etiqueta
        .trim_end_matches("-if00-port0")
        .trim_end_matches("-if00")
        .trim()
        .to_string();
    if etiqueta.is_empty() {
        None
    } else {
        Some(etiqueta)
    }
}

/// Fabrica la descripción de un dispositivo leyendo `/sys`.
fn describir(ruta: &Path, sys: &Path) -> Option<ImpresoraDetectada> {
    let nombre = ruta.file_name()?.to_string_lossy().to_string();
    let usb = nombre.starts_with("lp") || nombre.starts_with("usblp");

    let (vid, pid, clase, producto) = datos_de_sys(sys, &nombre);

    let (fabricante, perfil, es_impresora, nota) = {
        let fabricante = vid
            .and_then(fabricante_de)
            .map(|f| f.to_string())
            .or_else(|| producto.clone())
            .unwrap_or_else(|| "Desconocido".to_string());

        // Clase 07 = impresora USB. Es la única señal fiable.
        let es_impresora = clase == Some(7) || (usb && vid.is_some());

        let mut perfil = perfil_para(vid, usb);
        if usb {
            perfil.columnas = 48;
        }
        let nota = match (vid, clase) {
            (Some(_), Some(7)) => "Impresora USB detectada (clase impresora).".to_string(),
            (Some(_), _) => format!(
                "Puerto serie USB. Si es la impresora, revisa los baudios ({}).",
                perfil.baudios
            ),
            (None, _) => "Sin información USB: es un puerto de la placa o un enlace.".to_string(),
        };
        (fabricante, perfil, es_impresora, nota)
    };

    let transporte = if usb {
        Transporte::Archivo {
            ruta: ruta.to_path_buf(),
        }
    } else {
        Transporte::Serie {
            ruta: ruta.to_path_buf(),
            baudios: perfil.baudios,
        }
    };

    let etiqueta = if usb {
        format!("{fabricante} (USB {ruta})", ruta = ruta.display())
    } else {
        format!(
            "{fabricante} (serie {} a {} baudios)",
            ruta.display(),
            perfil.baudios
        )
    };

    Some(ImpresoraDetectada {
        ruta: ruta.display().to_string(),
        transporte,
        etiqueta,
        fabricante,
        vid,
        pid,
        es_impresora,
        perfil,
        nota,
    })
}

/// Busca VID/PID/clase/producto de un dispositivo en `/sys`.
///
/// El archivo `uevent` de un dispositivo USB trae líneas como
/// `PRODUCT=4b8/302/100` e `INTERFACE=7/1/2`. Según el kernel, ese archivo
/// puede estar en el directorio del dispositivo o un par de niveles arriba, así
/// que se sube hasta cinco veces antes de rendirse.
fn datos_de_sys(
    sys: &Path,
    nombre: &str,
) -> (Option<u16>, Option<u16>, Option<u8>, Option<String>) {
    let bases = [
        sys.join("class/usbmisc").join(nombre),
        sys.join("class/tty").join(nombre),
        sys.join("class/usbmisc").join(nombre).join("device"),
        sys.join("class/tty").join(nombre).join("device"),
    ];

    for base in bases {
        let mut dir = base.clone();
        for _ in 0..5 {
            if let Ok(texto) = std::fs::read_to_string(dir.join("uevent")) {
                let mut vid = None;
                let mut pid = None;
                let mut clase = None;
                for linea in texto.lines() {
                    if let Some(v) = linea.strip_prefix("PRODUCT=") {
                        let p: Vec<&str> = v.split('/').collect();
                        if p.len() >= 2 {
                            vid = u16::from_str_radix(p[0], 16).ok();
                            pid = u16::from_str_radix(p[1], 16).ok();
                        }
                    } else if let Some(v) = linea.strip_prefix("INTERFACE=") {
                        clase = v.split('/').next().and_then(|c| c.parse::<u8>().ok());
                    } else if let Some(v) = linea.strip_prefix("DEVINTERFACE=") {
                        clase = v.split('/').next().and_then(|c| c.parse::<u8>().ok());
                    }
                }
                if vid.is_some() {
                    let producto = std::fs::read_to_string(dir.join("product"))
                        .ok()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    return (vid, pid, clase, producto);
                }
            }
            if !dir.pop() {
                break;
            }
        }
    }
    (None, None, None, None)
}

/// Fabricantes que se reconocen con certeza por su identificador USB.
///
/// La lista es corta a propósito: están los que sabemos con seguridad. Todo lo
/// demás cae en «Desconocido» con el perfil genérico, que es el que funciona
/// con la inmensa mayoría de las térmicas que se venden en México (y que en su
/// mayoría se anuncian como clones de Epson).
fn fabricante_de(vid: u16) -> Option<&'static str> {
    Some(match vid {
        0x04B8 => "Epson",
        0x0519 => "Star Micronics",
        0x1504 => "Bixolon",
        0x1D90 => "Citizen",
        0x1A86 => "Adaptador CH340",
        0x0403 => "Adaptador FTDI",
        0x10C4 => "Adaptador CP210x",
        0x067B => "Adaptador Prolific",
        0x0483 => "Adaptador STMicro",
        _ => return None,
    })
}

/// Perfil sugerido según el fabricante que se reconoció.
fn perfil_para(vid: Option<u16>, usb: bool) -> Perfil {
    let base = if usb {
        Perfil::generica_80()
    } else {
        Perfil::generica_58()
    };
    match vid {
        Some(0x04B8) => Perfil::epson().con_columnas(base.columnas),
        Some(0x0519) => Perfil::star().con_columnas(base.columnas),
        Some(0x1504) => Perfil::bixolon().con_columnas(base.columnas),
        Some(0x1D90) => Perfil::citizen().con_columnas(base.columnas),
        // CH340, FTDI, CP210x y compañía: las chinas, a 9600.
        _ => base,
    }
}

#[cfg(windows)]
fn enumerar_windows() -> Vec<ImpresoraDetectada> {
    let mut lista: Vec<ImpresoraDetectada> = crate::winspool::listar()
        .into_iter()
        .map(|nombre| {
            let etiqueta = format!("{nombre} (Windows)");
            ImpresoraDetectada {
                ruta: nombre.clone(),
                transporte: Transporte::Spooler {
                    nombre: nombre.clone(),
                },
                etiqueta,
                fabricante: "Windows".to_string(),
                vid: None,
                pid: None,
                // PDF, Fax, XPS no son térmicas: no se proponen solas.
                es_impresora: !es_cola_virtual(&nombre),
                perfil: Perfil::generica_80(),
                nota: if es_cola_virtual(&nombre) {
                    "No es una térmica (PDF, Fax o XPS). Elígelo sólo si de verdad imprime tickets."
                        .to_string()
                } else {
                    "Impresora instalada en Windows. Se le imprime en modo RAW.".to_string()
                },
            }
        })
        .collect();
    lista.sort_by_key(|i| puntuacion_termica(&i.ruta));
    lista
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construye un árbol falso de /dev y /sys para probar la detección.
    fn arbol(dir: &Path) -> (PathBuf, PathBuf) {
        let dev = dir.join("dev");
        let sys = dir.join("sys");
        std::fs::create_dir_all(dev.join("usb")).unwrap();
        std::fs::create_dir_all(dev.join("serial/by-id")).unwrap();
        std::fs::create_dir_all(sys.join("class/usbmisc/lp0/device")).unwrap();
        std::fs::create_dir_all(sys.join("class/tty/ttyUSB0/device")).unwrap();

        // Una Epson TM-T20 por USB: clase 07 (impresora).
        std::fs::write(dev.join("usb/lp0"), b"").unwrap();
        std::fs::write(
            sys.join("class/usbmisc/lp0/device/uevent"),
            "DEVTYPE=usb_interface\nDRIVER=usblp\nPRODUCT=4b8/302/100\nTYPE=0/0/0\nINTERFACE=7/1/2\n",
        )
        .unwrap();
        std::fs::write(sys.join("class/usbmisc/lp0/device/product"), "TM-T20II\n").unwrap();

        // Una térmica china por adaptador CH340 (clase 255 = específica).
        std::fs::write(dev.join("ttyUSB0"), b"").unwrap();
        std::fs::write(
            sys.join("class/tty/ttyUSB0/device/uevent"),
            "DEVTYPE=usb_interface\nDRIVER=ch341\nPRODUCT=1a86/7523/254\nINTERFACE=255/0/0\n",
        )
        .unwrap();

        // Un enlace by-id que apunta al CH340. En Windows el symlink pide
        // privilegios que la caja no tiene: se deja un archivo normal (la
        // prueba que bautiza por by-id sólo corre en Unix).
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            "../../ttyUSB0",
            dev.join("serial/by-id/usb-1a86_USB-Serial-if00-port0"),
        )
        .unwrap();
        #[cfg(windows)]
        std::fs::write(dev.join("serial/by-id/usb-1a86_USB-Serial-if00-port0"), b"").unwrap();

        (dev, sys)
    }

    #[test]
    fn detecta_la_usb_y_la_serie_y_dice_de_quien_son() {
        let dir = std::env::temp_dir().join("pos-hw-detectar");
        let _ = std::fs::remove_dir_all(&dir);
        let (dev, sys) = arbol(&dir);

        let lista = enumerar_en(&dev, &sys);
        assert_eq!(lista.len(), 2, "esperaba la USB y la serie: {lista:?}");

        // La USB va primero: es la única que sabemos que es impresora.
        assert!(lista[0].es_impresora);
        assert_eq!(lista[0].fabricante, "Epson");
        assert_eq!(lista[0].vid, Some(0x04B8));
        assert_eq!(lista[0].pid, Some(0x0302));
        assert_eq!(lista[0].perfil, Perfil::epson().con_columnas(48));
        assert!(matches!(lista[0].transporte, Transporte::Archivo { .. }));

        // El CH340: puerto serie a 9600, perfil genérico.
        assert!(!lista[1].es_impresora);
        assert_eq!(lista[1].fabricante, "Adaptador CH340");
        assert_eq!(lista[1].vid, Some(0x1A86));
        assert_eq!(lista[1].perfil.baudios, 9600);
        assert!(matches!(lista[1].transporte, Transporte::Serie { .. }));
        // Y el enlace by-id lo bautizó (sólo en Unix: en Windows el árbol
        // falso lleva archivo normal porque el symlink pide privilegios).
        #[cfg(unix)]
        assert!(
            lista[1].etiqueta.contains("USB-Serial"),
            "{}",
            lista[1].etiqueta
        );
    }

    #[test]
    fn el_puerto_de_la_placa_no_se_propone_antes_que_el_usb() {
        let dir = std::env::temp_dir().join("pos-hw-detectar-orden");
        let _ = std::fs::remove_dir_all(&dir);
        let dev = dir.join("dev");
        std::fs::create_dir_all(&dev).unwrap();
        // Una PC típica: puerto de placa y adaptador USB, ninguno confirmado.
        std::fs::write(dev.join("ttyS0"), b"").unwrap();
        std::fs::write(dev.join("ttyUSB0"), b"").unwrap();

        let lista = enumerar_en(&dev, &dir.join("sys"));
        assert_eq!(lista.len(), 2);
        // El de placa va al final aunque sea alfabéticamente anterior.
        assert!(
            lista[0].ruta.ends_with("ttyUSB0"),
            "orden: {:?}",
            lista.iter().map(|i| &i.ruta).collect::<Vec<_>>()
        );
        assert!(lista[1].ruta.ends_with("ttyS0"));
        // Ninguna está confirmada: no se adivina.
        assert!(mejor(&lista).is_none());
    }

    #[test]
    fn el_enlace_by_id_no_cuenta_como_otra_impresora() {
        let dir = std::env::temp_dir().join("pos-hw-detectar2");
        let _ = std::fs::remove_dir_all(&dir);
        let (dev, sys) = arbol(&dir);
        // Hay 3 archivos en /dev (lp0, ttyUSB0 y el enlace) pero 2 dispositivos.
        let lista = enumerar_en(&dev, &sys);
        assert_eq!(lista.len(), 2);
        assert!(lista.iter().all(|i| !i.ruta.contains("by-id")));
    }

    #[test]
    fn sin_nada_conectado_no_se_inventa_una_impresora() {
        let dir = std::env::temp_dir().join("pos-hw-detectar-vacio");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dev")).unwrap();
        std::fs::create_dir_all(dir.join("sys")).unwrap();
        assert!(enumerar_en(&dir.join("dev"), &dir.join("sys")).is_empty());
        assert!(mejor(&[]).is_none());
    }

    #[test]
    fn un_puerto_de_la_placa_sin_datos_usb_aparece_pero_no_engana() {
        let dir = std::env::temp_dir().join("pos-hw-detectar-placa");
        let _ = std::fs::remove_dir_all(&dir);
        let dev = dir.join("dev");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(dev.join("ttyS0"), b"").unwrap();

        let lista = enumerar_en(&dev, &dir.join("sys"));
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].fabricante, "Desconocido");
        assert!(!lista[0].es_impresora);
        // No se propone sola: no está confirmada.
        assert!(mejor(&lista).is_none());
    }

    #[test]
    fn la_mejor_candidata_prefiere_una_impresora_de_verdad() {
        let serie = ImpresoraDetectada {
            ruta: "/dev/ttyUSB0".into(),
            transporte: Transporte::Serie {
                ruta: PathBuf::from("/dev/ttyUSB0"),
                baudios: 9600,
            },
            etiqueta: "x".into(),
            fabricante: "x".into(),
            vid: None,
            pid: None,
            es_impresora: false,
            perfil: Perfil::generica_80(),
            nota: String::new(),
        };
        let usb = ImpresoraDetectada {
            es_impresora: true,
            ruta: "/dev/usb/lp1".into(),
            ..serie.clone()
        };
        // Aunque la USB venga después en la lista, se prefiere.
        assert_eq!(
            mejor(&[serie.clone(), usb.clone()]).unwrap().ruta,
            "/dev/usb/lp1"
        );
        // Sin impresoras reales, no se adivina el modelo.
        assert!(
            mejor(&[serie]).is_none(),
            "un puerto serie no confirmado no se propone solo"
        );
    }

    #[test]
    fn las_colas_virtuales_de_windows_no_se_toman_por_termica() {
        assert!(es_cola_virtual("Microsoft Print to PDF"));
        assert!(es_cola_virtual("Fax"));
        assert!(es_cola_virtual("Microsoft XPS Document Writer"));
        assert!(es_cola_virtual("Send to OneNote 16"));
        assert!(!es_cola_virtual("POS-80"));
        assert!(!es_cola_virtual("EPSON TM-T20II"));
        assert!(!es_cola_virtual("USB001"));
        assert_eq!(puntuacion_termica("EPSON TM-T20II"), 0);
        assert_eq!(puntuacion_termica("POS-80"), 0);
        assert_eq!(puntuacion_termica("HP LaserJet"), 1);
        assert_eq!(puntuacion_termica("Microsoft Print to PDF"), 2);
        assert!(consejo_error_spooler(5, "POS-80").contains("pausada"));
        assert!(consejo_error_spooler(1801, "X").contains("Detectar"));
        assert!(consejo_error_spooler(1722, "X").contains("Cola de impresión"));
    }

    #[test]
    fn las_reglas_udev_del_instalador_mencionan_usbmisc() {
        let ruta = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/deb/lib/udev/rules.d/99-pos-impresora.rules");
        let texto = std::fs::read_to_string(&ruta)
            .unwrap_or_else(|e| panic!("falta {}: {e}", ruta.display()));
        assert!(texto.contains("usbmisc"), "sin usbmisc: {texto}");
        assert!(texto.contains("dialout"), "sin dialout: {texto}");
        assert!(texto.contains("uaccess"), "sin uaccess: {texto}");
        assert!(texto.contains("lp*"), "sin lp*: {texto}");
    }

    #[test]
    fn los_fabricantes_conocidos_se_reconocen_por_su_vid() {
        assert_eq!(fabricante_de(0x04B8), Some("Epson"));
        assert_eq!(fabricante_de(0x1A86), Some("Adaptador CH340"));
        assert_eq!(fabricante_de(0x0403), Some("Adaptador FTDI"));
        // Uno que no conocemos: mejor decirlo que inventar.
        assert_eq!(fabricante_de(0x1234), None);
    }

    #[test]
    fn el_perfil_sigue_al_fabricante() {
        assert_eq!(perfil_para(Some(0x04B8), true).baudios, 115200);
        assert_eq!(perfil_para(Some(0x1504), true).baudios, 115200);
        assert_eq!(perfil_para(Some(0x1A86), false).baudios, 9600);
        assert_eq!(perfil_para(None, true).columnas, 48);
        assert_eq!(perfil_para(None, false).columnas, 32);
    }

    #[test]
    fn el_nombre_del_enlace_by_id_queda_legible() {
        let p = Path::new("/dev/serial/by-id/usb-1a86_USB2.0-Serial-if00-port0");
        assert_eq!(nombre_del_enlace(p).unwrap(), "USB2.0-Serial");
    }

    /// La detección real del equipo nunca debe tronar, tenga o no impresora.
    #[test]
    fn enumerar_en_este_equipo_no_truena() {
        let lista = enumerar();
        for i in &lista {
            assert!(!i.ruta.is_empty());
            assert!(!i.etiqueta.is_empty());
        }
    }
}
