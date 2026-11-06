//! Cómo llegan los bytes a la impresora: los tres caminos reales.
//!
//! Una térmica se conecta al mostrador de tres formas, y las tres están aquí:
//!
//!   * **USB con clase impresora** (`/dev/usb/lp0`, `USB001`): aparece como
//!     dispositivo del sistema. Se le escribe directo, sin driver del
//!     fabricante, sin CUPS y sin gastar RAM. Es el caso más común.
//!   * **Puerto serie / adaptador USB-serial** (`/dev/ttyUSB0`, `COM3`): así
//!     se conectan las chinas (CH340, FTDI, Prolific) y las Epson de modelo
//!     viejo. Hay que configurar `termios`: si no, los baudios están mal y el
//!     ticket sale como basura... o no sale.
//!   * **Red** (`192.168.1.50:9100`): las impresoras de cocina y las que están
//!     en otra caja. Es TCP sin protocolo: mandas bytes y ya.
//!
//! En Windows se suma el **spooler**: se manda el ticket en modo RAW al nombre
//! de la impresora, que es lo que hace cualquier programa de punto de venta.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::perfil::Perfil;
use crate::printer::{HwError, HwResult, Printer};

/// Puerto estándar de impresión en red (JetDirect / "raw" en puerto 9100).
pub const PUERTO_RAW: u16 = 9100;

/// Un camino por donde salir a la impresora.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transporte {
    /// Dispositivo del sistema: `/dev/usb/lp0`, `/dev/usblp0`, un archivo de
    /// prueba, o `\\.\COM3` en Windows cuando ya está configurado.
    Archivo { ruta: PathBuf },
    /// Puerto serie con baudios explícitos.
    Serie { ruta: PathBuf, baudios: u32 },
    /// Impresora de red: `192.168.1.50:9100`.
    Red { host: String, puerto: u16 },
    /// Nombre de impresora de Windows ("POS-80").
    Spooler { nombre: String },
}

impl Transporte {
    /// Adivina el transporte a partir de lo que escribió el encargado.
    ///
    /// Reglas, en orden:
    ///   * `host:puerto` o una dirección IP → red.
    ///   * `/dev/usb/lp*`, `/dev/usblp*` → USB clase impresora.
    ///   * `/dev/tty*`, `COM1..COM9` → serie (con los baudios indicados).
    ///   * Ruta de archivo de Windows (`C:\…`, `C:/…`, o un archivo que ya
    ///     existe) → archivo, igual que en Linux. Las pruebas imprimen a un
    ///     `.bin` temporal y si no, en Windows todo sería spooler.
    ///   * Cualquier otra cosa → dispositivo de archivo en Linux y, en Windows,
    ///     spooler (porque allí lo normal es el nombre de la impresora).
    pub fn desde_texto(s: &str, baudios: u32) -> Transporte {
        let s = s.trim();
        if let Some((host, puerto)) = partir_host_puerto(s) {
            return Transporte::Red { host, puerto };
        }
        if s.is_empty() {
            return Transporte::Archivo {
                ruta: PathBuf::new(),
            };
        }
        let min = s.to_ascii_lowercase();
        if min.starts_with("/dev/usb/lp") || min.starts_with("/dev/usblp") {
            Transporte::Archivo {
                ruta: PathBuf::from(s),
            }
        } else if min.starts_with("/dev/tty") || min.starts_with("/dev/serial/") || es_com(&min) {
            Transporte::Serie {
                ruta: PathBuf::from(s),
                baudios: if baudios == 0 { 9600 } else { baudios },
            }
        } else if cfg!(windows) && es_ruta_archivo_windows(s) {
            Transporte::Archivo {
                ruta: PathBuf::from(s),
            }
        } else if cfg!(windows) {
            Transporte::Spooler {
                nombre: s.to_string(),
            }
        } else {
            Transporte::Archivo {
                ruta: PathBuf::from(s),
            }
        }
    }

    /// Etiqueta legible para la bitácora y la pantalla.
    pub fn etiqueta(&self) -> String {
        match self {
            Transporte::Archivo { ruta } => format!("USB {}", ruta.display()),
            Transporte::Serie { ruta, baudios } => {
                format!("Serie {} a {baudios} baudios", ruta.display())
            }
            Transporte::Red { host, puerto } => format!("Red {host}:{puerto}"),
            Transporte::Spooler { nombre } => format!("Windows «{nombre}»"),
        }
    }

    /// Baudios que usa este transporte (0 si no aplica).
    pub fn baudios(&self) -> u32 {
        match self {
            Transporte::Serie { baudios, .. } => *baudios,
            _ => 0,
        }
    }

    /// Abre el transporte y devuelve algo con lo que se puede imprimir.
    pub fn abrir(&self) -> HwResult<Box<dyn Printer>> {
        match self {
            Transporte::Archivo { ruta } => {
                if ruta.as_os_str().is_empty() {
                    return Err(HwError::Puerto(
                        "no hay ninguna impresora configurada".to_string(),
                    ));
                }
                Ok(Box::new(PuertoArchivo::abrir(ruta)?))
            }
            Transporte::Serie { ruta, baudios } => {
                Ok(Box::new(PuertoSerie::abrir(ruta, *baudios)?))
            }
            Transporte::Red { host, puerto } => Ok(Box::new(PuertoRed::abrir(host, *puerto)?)),
            Transporte::Spooler { nombre } => Ok(Box::new(PuertoSpooler::abrir(nombre)?)),
        }
    }

    /// Imprime con el perfil puesto: abre el puerto, manda los bytes por trozos
    /// y cierra. Es lo que usa la aplicación; nada más debería abrir el puerto.
    pub fn imprimir(&self, perfil: &Perfil, bytes: &[u8]) -> HwResult<()> {
        let mut p = self.abrir()?;
        let trozo = perfil.trozo.max(64);
        let pausa = perfil.pausa_efectiva_ms(self.baudios());
        escribir_troceado(&mut *p, bytes, trozo, pausa)
    }
}

/// Separa "192.168.1.50:9100" o "impresora.local" en host y puerto.
fn partir_host_puerto(s: &str) -> Option<(String, u16)> {
    let s = s.trim();
    if s.is_empty() || s.contains('/') || s.contains('\\') {
        return None;
    }
    if let Some((host, puerto)) = s.rsplit_once(':') {
        if let Ok(p) = puerto.trim().parse::<u16>() {
            if p > 0 && !host.is_empty() {
                return Some((host.trim().to_string(), p));
            }
        }
        return None;
    }
    // ¿Es una IP o un nombre de red sin puerto? Se usa el 9100.
    let parece_ip = s.chars().next().is_some_and(|c| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_digit() || c == '.')
        && s.matches('.').count() == 3;
    let parece_host = s.ends_with(".local") || s.contains(".local:");
    if parece_ip || parece_host {
        return Some((s.to_string(), PUERTO_RAW));
    }
    None
}

/// ¿Es una ruta de archivo de Windows y no un nombre de impresora?
///
/// `C:\Temp\ticket.bin`, `C:/x/y.txt` o cualquier archivo que ya exista van
/// a archivo (las pruebas imprimen a Temp). Un nombre de spooler (`POS-80`,
/// `EPSON TM-T20II`) y un recurso compartido (`\\PC\POS-80`) no tienen letra
/// de unidad ni extensión de archivo, así que siguen yendo al spooler.
#[cfg(windows)]
fn es_ruta_archivo_windows(s: &str) -> bool {
    let t = s.trim();
    let b = t.as_bytes();
    // Letra de unidad: `C:\…` o `C:/…`.
    if b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
        return true;
    }
    // Existe como archivo (la prueba lo crea antes de imprimir).
    if Path::new(t).exists() && !t.starts_with("\\\\") {
        return true;
    }
    // Termina en extensión de archivo y trae separador: casi seguro es ruta.
    let minus = t.to_ascii_lowercase();
    if (minus.ends_with(".bin") || minus.ends_with(".txt") || minus.ends_with(".dat"))
        && (t.contains('\\') || t.contains('/'))
    {
        return true;
    }
    false
}

fn es_com(s: &str) -> bool {
    if !s.starts_with("com") {
        return false;
    }
    let digitos: String = s
        .chars()
        .skip(3)
        .take_while(|c| c.is_ascii_digit())
        .collect();
    !digitos.is_empty() && digitos.len() <= 3
}

/// Escribe por trozos con una pausa opcional entre ellos.
///
/// ¿Por qué? Porque las térmicas chinas tienen búfers de 1 a 4 KB. Un ticket
/// con QR pasa fácil los 2 KB; mandado de un golpe por un adaptador serie a
/// 9600 baudios, se pierde el final: el cliente se queda sin corte y con el
/// total a medias. Trocear es la diferencia entre un ticket entero y uno roto.
pub fn escribir_troceado(
    p: &mut dyn Printer,
    bytes: &[u8],
    trozo: usize,
    pausa_ms: u64,
) -> HwResult<()> {
    let trozo = trozo.max(16);
    for pedazo in bytes.chunks(trozo) {
        p.write(pedazo)?;
        if pausa_ms > 0 {
            std::thread::sleep(Duration::from_millis(pausa_ms));
        }
    }
    Ok(())
}

/// Dispositivo USB de clase impresora (o archivo, para pruebas).
#[derive(Debug)]
pub struct PuertoArchivo {
    ruta: PathBuf,
}

impl PuertoArchivo {
    pub fn abrir(ruta: &Path) -> HwResult<Self> {
        // Se abre y se cierra para fallar pronto: si la impresora está apagada,
        // el error aparece aquí y no cuando el cliente ya está esperando.
        File::options().append(true).open(ruta).map_err(|e| {
            HwError::Puerto(format!(
                "no se pudo abrir {}: {}. {}",
                ruta.display(),
                e,
                consejo_permiso(&e, ruta)
            ))
        })?;
        Ok(PuertoArchivo {
            ruta: ruta.to_path_buf(),
        })
    }
}

/// Distingue permiso, desconectada y ocupada. No adivina el modelo.
///
/// En una tienda real el caso más común y el más desconcertante es el
/// permiso: la impresora está ahí, prendida, y el programa dice que no.
/// En Linux el dispositivo pertenece al grupo `lp` (USB) o `dialout`
/// (serie). El instalador ya mete al usuario, pero si se instaló a mano
/// o se cambió de usuario, hay que decirlo con todas sus letras.
fn consejo_permiso(e: &std::io::Error, ruta: &Path) -> String {
    // Se revisa también el texto: según el kernel y la libc, "Permission
    // denied" no siempre llega como `ErrorKind::PermissionDenied`.
    let texto = e.to_string().to_ascii_lowercase();
    if e.kind() == std::io::ErrorKind::PermissionDenied || texto.contains("permission denied") {
        return consejo_de_grupo(ruta);
    }
    if e.kind() == std::io::ErrorKind::NotFound
        || texto.contains("no such file")
        || texto.contains("cannot find")
        || texto.contains("the system cannot find")
    {
        return "No está ese puerto. Pulsa Detectar o corre: pos --impresoras. ¿Está conectada y encendida?".to_string();
    }
    if e.kind() == std::io::ErrorKind::ResourceBusy
        || texto.contains("busy")
        || texto.contains("in use")
        || texto.contains("device or resource busy")
    {
        return "Otro programa la está usando (CUPS o el driver del fabricante). Ciérralo y vuelve a probar.".to_string();
    }
    "¿Está encendida y conectada? Pulsa Detectar o corre: pos --impresoras.".to_string()
}

fn consejo_de_grupo(ruta: &Path) -> String {
    #[cfg(windows)]
    {
        let _ = ruta;
        "En Windows: ¿está instalada en Impresoras y escáneres? Si sí, usa su nombre, no una ruta de /dev.".to_string()
    }
    #[cfg(not(windows))]
    {
        let nombre = ruta
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let grupo = if nombre.starts_with("ttyUSB")
            || nombre.starts_with("ttyACM")
            || nombre.starts_with("ttyS")
        {
            "dialout"
        } else {
            "lp"
        };
        format!(
            "Falta permiso: corre  sudo usermod -a -G {grupo} $USER  \
             (o reinstala con el .deb, que lo hace solo) y cierra sesión."
        )
    }
}

impl Printer for PuertoArchivo {
    fn name(&self) -> &str {
        self.ruta.to_str().unwrap_or("usb")
    }

    fn write(&mut self, bytes: &[u8]) -> HwResult<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.ruta)
            .map_err(|e| {
                HwError::Puerto(format!(
                    "se cerró {}: {}. {}",
                    self.ruta.display(),
                    e,
                    consejo_permiso(&e, &self.ruta)
                ))
            })?;
        f.write_all(bytes).map_err(|e| {
            HwError::Puerto(format!(
                "no se pudo imprimir en {}: {}. {}",
                self.ruta.display(),
                e,
                consejo_permiso(&e, &self.ruta)
            ))
        })?;
        f.flush().map_err(|e| HwError::Puerto(e.to_string()))
    }
}

/// Puerto serie configurado a mano con `termios`: 8N1, sin control de flujo.
#[derive(Debug)]
pub struct PuertoSerie {
    ruta: PathBuf,
    baudios: u32,
}

impl PuertoSerie {
    pub fn abrir(ruta: &Path, baudios: u32) -> HwResult<Self> {
        #[cfg(windows)]
        configurar_modo_windows(ruta, baudios)?;
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(ruta)
            .map_err(|e| {
                HwError::Puerto(format!(
                    "no se pudo abrir el puerto {}: {}. {}",
                    ruta.display(),
                    e,
                    consejo_permiso(&e, ruta)
                ))
            })?;
        configurar_serie(&f, baudios)?;
        Ok(PuertoSerie {
            ruta: ruta.to_path_buf(),
            baudios,
        })
    }

    pub fn baudios(&self) -> u32 {
        self.baudios
    }
}

impl Printer for PuertoSerie {
    fn name(&self) -> &str {
        self.ruta.to_str().unwrap_or("serie")
    }

    fn write(&mut self, bytes: &[u8]) -> HwResult<()> {
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.ruta)
            .map_err(|e| {
                HwError::Puerto(format!(
                    "se cerró {}: {}. {}",
                    self.ruta.display(),
                    e,
                    consejo_permiso(&e, &self.ruta)
                ))
            })?;
        configurar_serie(&f, self.baudios)?;
        f.write_all(bytes).map_err(|e| {
            HwError::Puerto(format!(
                "no se pudo imprimir en {}: {}. {}",
                self.ruta.display(),
                e,
                consejo_permiso(&e, &self.ruta)
            ))
        })?;
        f.flush().map_err(|e| HwError::Puerto(e.to_string()))
    }
}

/// Deja el puerto en 8N1 a los baudios pedidos, sin control de flujo.
///
/// Recibe un `&File` (no una ruta) a propósito: así se puede probar de verdad
/// sobre una pseudoterminal, sin tener una impresora enfrente.
#[cfg(unix)]
pub fn configurar_serie(f: &File, baudios: u32) -> HwResult<()> {
    use std::os::fd::AsRawFd;

    let constante = velocidad_constante(baudios).ok_or_else(|| {
        HwError::Puerto(format!(
            "{baudios} baudios: usa uno de 9600, 19200, 38400, 57600 o 115200"
        ))
    })?;

    unsafe {
        let fd = f.as_raw_fd();
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return Err(HwError::Puerto(format!(
                "el puerto no acepta la configuración serie: {}",
                std::io::Error::last_os_error()
            )));
        }
        libc::cfmakeraw(&mut t); // sin eco, sin conversiones de salida
        t.c_cflag |= libc::CLOCAL | libc::CREAD;
        t.c_cflag &= !libc::CSIZE;
        t.c_cflag |= libc::CS8; // 8 bits de datos
        t.c_cflag &= !libc::PARENB; // sin paridad
        t.c_cflag &= !libc::CSTOPB; // 1 bit de paro
                                    // Sin control de flujo ni por hardware ni por software: con XON/XOFF
                                    // activado, un 0x11 o un 0x13 dentro del ticket congela la impresora.
        #[cfg(not(target_os = "macos"))]
        {
            t.c_cflag &= !libc::CRTSCTS;
        }
        t.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);
        t.c_cc[libc::VMIN] = 0;
        t.c_cc[libc::VTIME] = 10;
        if libc::cfsetispeed(&mut t, constante) != 0 || libc::cfsetospeed(&mut t, constante) != 0 {
            return Err(HwError::Puerto(format!("{baudios} baudios no válidos")));
        }
        if libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
            return Err(HwError::Puerto(format!(
                "no se pudo configurar el puerto: {}",
                std::io::Error::last_os_error()
            )));
        }
        let _ = libc::tcflush(fd, libc::TCIOFLUSH);
    }
    Ok(())
}

/// En Windows el puerto serie se configura con la herramienta `mode`, que viene
/// con el sistema. Se hace a propósito: la alternativa es declarar a mano la
/// estructura `DCB` con sus campos de bits y, si se declara mal, se corrompe
/// memoria en la caja del cliente. `mode` es más lento pero no puede romper
/// nada, y se ejecuta una sola vez al abrir el puerto.
#[cfg(windows)]
fn configurar_modo_windows(ruta: &Path, baudios: u32) -> HwResult<()> {
    let nombre = ruta.to_string_lossy().trim().to_string();
    let salida = std::process::Command::new("mode")
        .arg(format!("{nombre}:"))
        .arg(format!("BAUD={baudios}"))
        .arg("PARITY=N")
        .arg("DATA=8")
        .arg("STOP=1")
        .output();
    match salida {
        Ok(o) if o.status.success() => Ok(()),
        Ok(_) => Err(HwError::Puerto(format!(
            "no se pudo configurar {nombre} a {baudios} baudios"
        ))),
        Err(e) => Err(HwError::Puerto(format!("no se pudo ejecutar «mode»: {e}"))),
    }
}

#[cfg(windows)]
pub fn configurar_serie(_f: &File, _baudios: u32) -> HwResult<()> {
    // El modo ya lo puso `configurar_modo_windows` antes de abrir.
    Ok(())
}

#[cfg(unix)]
fn velocidad_constante(baudios: u32) -> Option<libc::speed_t> {
    Some(match baudios {
        1200 => libc::B1200,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        230400 => libc::B230400,
        _ => return None,
    })
}

/// Impresora de red: TCP en crudo al puerto 9100.
#[derive(Debug)]
pub struct PuertoRed {
    host: String,
    puerto: u16,
    flujo: Option<TcpStream>,
}

impl PuertoRed {
    fn direccion(host: &str, puerto: u16) -> HwResult<std::net::SocketAddr> {
        (host, puerto)
            .to_socket_addrs()
            .map_err(|e| HwError::Puerto(format!("no existe la impresora {host}:{puerto}: {e}")))?
            .next()
            .ok_or_else(|| HwError::Puerto(format!("la impresora {host}:{puerto} no responde")))
    }

    pub fn abrir(host: &str, puerto: u16) -> HwResult<Self> {
        // Un intento de conexión con tiempo límite: si la impresora está
        // apagada, el cajero se entera en 3 segundos y no en 30.
        let dir = PuertoRed::direccion(host, puerto)?;
        let flujo = TcpStream::connect_timeout(&dir, Duration::from_secs(3)).map_err(|e| {
            HwError::Puerto(format!(
                "no se pudo conectar con {host}:{puerto}: {e}. ¿Está encendida y en la misma red?"
            ))
        })?;
        flujo
            .set_nodelay(true)
            .map_err(|e| HwError::Puerto(e.to_string()))?;
        Ok(PuertoRed {
            host: host.to_string(),
            puerto,
            flujo: Some(flujo),
        })
    }
}

impl Printer for PuertoRed {
    fn name(&self) -> &str {
        &self.host
    }

    fn write(&mut self, bytes: &[u8]) -> HwResult<()> {
        let flujo = self
            .flujo
            .as_mut()
            .ok_or_else(|| HwError::Puerto("la conexión se cerró".to_string()))?;
        flujo.write_all(bytes).map_err(|e| {
            HwError::Puerto(format!(
                "se cortó la red a {}:{} a media impresión: {e}",
                self.host, self.puerto
            ))
        })?;
        flujo.flush().map_err(|e| HwError::Puerto(e.to_string()))
    }
}

/// Impresora de Windows por el spooler, en modo RAW.
///
/// El modo RAW es la clave: le dice al spooler que no interprete nada, que
/// mande los bytes ESC/POS tal cual. Sin eso, Windows intenta "dibujar" el
/// ticket y sale una hoja con líneas de caracteres raros.
#[derive(Debug)]
pub struct PuertoSpooler {
    nombre: String,
}

impl PuertoSpooler {
    pub fn abrir(nombre: &str) -> HwResult<Self> {
        if nombre.trim().is_empty() {
            return Err(HwError::Puerto(
                "no hay ninguna impresora de Windows configurada".to_string(),
            ));
        }
        Ok(PuertoSpooler {
            nombre: nombre.to_string(),
        })
    }
}

impl Printer for PuertoSpooler {
    fn name(&self) -> &str {
        &self.nombre
    }

    #[cfg(windows)]
    fn write(&mut self, bytes: &[u8]) -> HwResult<()> {
        crate::winspool::escribir_raw(&self.nombre, bytes)
            .map_err(|e| HwError::Puerto(format!("{}: {e}", self.nombre)))
    }

    #[cfg(not(windows))]
    fn write(&mut self, _bytes: &[u8]) -> HwResult<()> {
        Err(HwError::Puerto(format!(
            "«{}» es una impresora de Windows y esto es Linux: usa /dev/usb/lp0 o un puerto serie",
            self.nombre
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_transporte_se_adivina_de_lo_que_escribe_el_encargado() {
        assert_eq!(
            Transporte::desde_texto("/dev/usb/lp0", 0),
            Transporte::Archivo {
                ruta: PathBuf::from("/dev/usb/lp0")
            }
        );
        assert_eq!(
            Transporte::desde_texto("/dev/ttyUSB0", 0),
            Transporte::Serie {
                ruta: PathBuf::from("/dev/ttyUSB0"),
                baudios: 9600
            }
        );
        assert_eq!(
            Transporte::desde_texto("/dev/ttyUSB0", 115200),
            Transporte::Serie {
                ruta: PathBuf::from("/dev/ttyUSB0"),
                baudios: 115200
            }
        );
        assert_eq!(
            Transporte::desde_texto("192.168.1.50:9100", 0),
            Transporte::Red {
                host: "192.168.1.50".to_string(),
                puerto: 9100
            }
        );
        // Una IP sin puerto usa el 9100.
        assert_eq!(
            Transporte::desde_texto("192.168.1.50", 0),
            Transporte::Red {
                host: "192.168.1.50".to_string(),
                puerto: PUERTO_RAW
            }
        );
        // COM3 en Windows...
        assert_eq!(
            Transporte::desde_texto("COM3", 19200),
            Transporte::Serie {
                ruta: PathBuf::from("COM3"),
                baudios: 19200
            }
        );
        assert!(es_com("com12"));
        assert!(!es_com("comparar"));
    }

    #[test]
    fn una_ruta_de_linux_no_se_confunde_con_una_de_red() {
        // "/dev/usb/lp0" tiene barras: no es host:puerto.
        assert!(partir_host_puerto("/dev/usb/lp0").is_none());
        assert!(partir_host_puerto("").is_none());
        assert_eq!(
            partir_host_puerto("cocina.local:9100"),
            Some(("cocina.local".to_string(), 9100))
        );
    }

    #[test]
    fn la_etiqueta_dice_por_donde_sale_el_ticket() {
        let t = Transporte::Serie {
            ruta: PathBuf::from("/dev/ttyUSB0"),
            baudios: 9600,
        };
        assert_eq!(t.etiqueta(), "Serie /dev/ttyUSB0 a 9600 baudios");
        assert_eq!(t.baudios(), 9600);
        assert_eq!(
            Transporte::Red {
                host: "h".into(),
                puerto: 9100
            }
            .baudios(),
            0
        );
    }

    #[test]
    fn el_puerto_de_archivo_escribe_y_acumula() {
        let dir = std::env::temp_dir().join("pos-hw-transporte");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("impresora");
        let _ = std::fs::remove_file(&ruta);
        std::fs::write(&ruta, b"").unwrap(); // "conectamos" la impresora

        {
            let mut p = PuertoArchivo::abrir(&ruta).unwrap();
            p.write(b"uno").unwrap();
            p.write(b"dos").unwrap();
        }
        assert_eq!(std::fs::read(&ruta).unwrap(), b"unodos");
        std::fs::remove_file(&ruta).unwrap();
    }

    #[test]
    fn imprimir_por_trozos_entrega_todo_el_ticket() {
        struct Recolector(Vec<u8>);
        impl Printer for Recolector {
            fn name(&self) -> &str {
                "recolector"
            }
            fn write(&mut self, bytes: &[u8]) -> HwResult<()> {
                self.0.extend_from_slice(bytes);
                Ok(())
            }
        }
        let mut r = Recolector(Vec::new());
        let ticket: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        escribir_troceado(&mut r, &ticket, 64, 0).unwrap();
        assert_eq!(r.0, ticket, "el troceado perdió bytes");
    }

    #[test]
    fn el_puerto_de_archivo_avisa_si_no_existe() {
        let err = PuertoArchivo::abrir(Path::new("/dev/usb/lpQueNoExiste")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no se pudo abrir"), "mensaje pobre: {msg}");
        assert!(msg.contains("encendida"), "falta el consejo: {msg}");
        assert!(
            msg.contains("impresoras") || msg.contains("Detectar"),
            "sin pista de cómo listar: {msg}"
        );
    }

    #[test]
    fn el_consejo_distingue_permiso_de_desconectada() {
        use std::io::{Error, ErrorKind};
        let permiso = Error::from(ErrorKind::PermissionDenied);
        // En Linux el permiso dice el grupo (dialout/lp + usermod); en
        // Windows dice que revise Impresoras y escáneres. El consejo cambia
        // con la plataforma, pero nunca se queda en "permission denied".
        #[cfg(unix)]
        {
            // Un puerto serie pide el grupo dialout...
            let m = consejo_permiso(&permiso, Path::new("/dev/ttyUSB0"));
            assert!(m.contains("dialout"), "sin el grupo correcto: {m}");
            assert!(m.contains("usermod"), "sin el comando: {m}");
            // ...y una impresora USB pide lp.
            let m = consejo_permiso(&permiso, Path::new("/dev/usb/lp0"));
            assert!(m.contains("lp"), "sin el grupo correcto: {m}");
        }
        #[cfg(windows)]
        {
            let m = consejo_permiso(&permiso, Path::new("/dev/ttyUSB0"));
            assert!(
                m.contains("Windows") || m.contains("Impresoras"),
                "en Windows debe mandar a Impresoras y escáneres: {m}"
            );
        }
        // Si no está, dice cómo listar — no se queda en "permission denied".
        let m = consejo_permiso(&Error::from(ErrorKind::NotFound), Path::new("/dev/usb/lp0"));
        assert!(m.contains("encendida"), "consejo equivocado: {m}");
        assert!(
            m.contains("impresoras") || m.contains("Detectar"),
            "desconectada debe decir cómo listar: {m}"
        );
        let m = consejo_permiso(
            &Error::new(ErrorKind::ResourceBusy, "Device or resource busy"),
            Path::new("/dev/usb/lp0"),
        );
        assert!(m.contains("CUPS") || m.contains("usando"), "ocupada: {m}");
    }

    #[test]
    fn imprimir_con_perfil_pasa_por_el_transporte() {
        let dir = std::env::temp_dir().join("pos-hw-transporte2");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("impresora2");
        let _ = std::fs::remove_file(&ruta);
        std::fs::write(&ruta, b"").unwrap();
        let t = Transporte::Archivo { ruta: ruta.clone() };
        let mut perfil = Perfil::generica_80();
        perfil.trozo = 64;
        perfil.pausa_ms = 0;
        t.imprimir(&perfil, b"ticket de prueba").unwrap();
        assert_eq!(std::fs::read(&ruta).unwrap(), b"ticket de prueba");
        std::fs::remove_file(&ruta).unwrap();
    }

    /// Prueba de verdad sobre una pseudoterminal: abre un par maestro/esclavo,
    /// configura el esclavo como puerto serie y comprueba que los baudios se
    /// aplicaron y que los bytes llegan intactos al otro lado.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn el_puerto_serie_se_configura_de_verdad_8n1_y_sin_flujo() {
        use std::os::fd::{AsRawFd, FromRawFd};

        let mut maestro: libc::c_int = 0;
        let mut esclavo: libc::c_int = 0;
        let r = unsafe {
            libc::openpty(
                &mut maestro,
                &mut esclavo,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(r, 0, "no se pudo abrir la pseudoterminal");
        let f = unsafe { File::from_raw_fd(esclavo) };

        configurar_serie(&f, 19200).expect("la configuración falló");

        // 1. Los baudios quedaron puestos.
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        unsafe { libc::tcgetattr(f.as_raw_fd(), &mut t) };
        assert_eq!(unsafe { libc::cfgetispeed(&t) }, libc::B19200);
        assert_eq!(unsafe { libc::cfgetospeed(&t) }, libc::B19200);

        // 2. 8 bits de datos, sin paridad, 1 bit de paro.
        assert_eq!(t.c_cflag & libc::CSIZE, libc::CS8);
        assert_eq!(t.c_cflag & libc::PARENB, 0, "quedó paridad activada");
        assert_eq!(t.c_cflag & libc::CSTOPB, 0, "quedaron 2 bits de paro");

        // 3. Sin control de flujo por software (si no, un 0x13 congela el ticket).
        assert_eq!(t.c_iflag & (libc::IXON | libc::IXOFF | libc::IXANY), 0);

        // 4. Y los bytes escritos salen por el otro lado.
        let mut abierto = f.try_clone().unwrap();
        use std::io::Write;
        abierto
            .write_all(&[0x1B, b'@', b'H', b'o', b'l', b'a'])
            .unwrap();
        abierto.flush().unwrap();
        let mut buf = [0u8; 16];
        let n = unsafe { libc::read(maestro, buf.as_mut_ptr() as *mut libc::c_void, 16) };
        assert_eq!(&buf[..n as usize], &[0x1B, b'@', b'H', b'o', b'l', b'a']);

        unsafe { libc::close(maestro) };
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn una_velocidad_rara_se_rechaza_con_un_mensaje_claro() {
        let mut maestro: libc::c_int = 0;
        let mut esclavo: libc::c_int = 0;
        use std::os::fd::FromRawFd;
        unsafe {
            libc::openpty(
                &mut maestro,
                &mut esclavo,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        let f = unsafe { File::from_raw_fd(esclavo) };
        let err = configurar_serie(&f, 12345).unwrap_err();
        assert!(
            err.to_string().contains("9600"),
            "no sugiere velocidades: {err}"
        );
        unsafe { libc::close(maestro) };
    }

    /// La impresora de red se prueba con un socket de verdad en loopback.
    #[test]
    fn el_puerto_de_red_manda_el_ticket_por_el_socket() {
        let escucha = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto = escucha.local_addr().unwrap().port();
        let servidor = std::thread::spawn(move || {
            let (mut s, _) = escucha.accept().unwrap();
            use std::io::Read;
            let mut buf = [0u8; 256];
            let n = s.read(&mut buf).unwrap();
            buf[..n].to_vec()
        });
        let mut p = PuertoRed::abrir("127.0.0.1", puerto).unwrap();
        p.write(&[0x1B, b'@', b'A', b'B', b'C']).unwrap();
        let recibido = servidor.join().unwrap();
        assert_eq!(recibido, vec![0x1B, b'@', b'A', b'B', b'C']);
    }

    #[test]
    fn una_impresora_de_red_apagada_avisa_en_tres_segundos() {
        // Puerto cerrado en loopback: debe fallar rápido y con consejo.
        let escucha = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto_cerrado = escucha.local_addr().unwrap().port();
        drop(escucha);
        let inicio = std::time::Instant::now();
        let err = PuertoRed::abrir("127.0.0.1", puerto_cerrado).unwrap_err();
        assert!(
            inicio.elapsed() < Duration::from_secs(10),
            "tardó demasiado"
        );
        assert!(
            err.to_string().contains("encendida"),
            "falta el consejo: {err}"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn el_spooler_de_windows_avisa_que_esto_es_linux() {
        let mut p = PuertoSpooler::abrir("POS-80").unwrap();
        let err = p.write(b"x").unwrap_err();
        assert!(err.to_string().contains("Linux"), "mensaje pobre: {err}");
        assert!(PuertoSpooler::abrir("  ").is_err());
    }
}

#[cfg(all(test, unix))]
mod depuracion {
    use super::*;
    #[test]
    fn que_error_llega_cuando_no_hay_permiso() {
        let dir = std::env::temp_dir().join("pos-hw-permiso");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("sin-permiso");
        std::fs::write(&ruta, b"").unwrap();
        std::fs::set_permissions(&ruta, std::os::unix::fs::PermissionsExt::from_mode(0o000))
            .unwrap();
        let e = std::fs::OpenOptions::new()
            .append(true)
            .open(&ruta)
            .unwrap_err();
        println!("kind={:?} display={}", e.kind(), e);
        println!("consejo -> {}", consejo_permiso(&e, &ruta));
        std::fs::set_permissions(&ruta, std::os::unix::fs::PermissionsExt::from_mode(0o644))
            .unwrap();
    }
}
