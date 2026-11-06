# Punto de Venta en Rust

POS de mostrador para retail, pensado para **computadoras viejas**: arranca en
menos de un segundo y vive cómodo con menos de 100 MB de RAM.

```
Rust nativo + egui (OpenGL)  ──►  un solo ejecutable
SQLite embebida (WAL)        ──►  offline, resistente a apagones
ESC/POS directo              ──►  ticket térmico sin drivers
```

Documento completo de producto, MVP y roadmap: **[`../POS-Rust-Documento.md`](../POS-Rust-Documento.md)**

---

## Estado actual (Fases 0, 1, 3 y 4 parciales)

| Listo | Pendiente |
|-------|-----------|
| Workspace con 7 crates | Multi-caja y sync (Fase 6) |
| Dinero en centavos enteros (`i64`) | App móvil nativa (el servidor y `cliente_movil.py` ya atienden) |
| Motor de IVA (16/8/0 %, exento) | |
| Venta con pagos mixtos y cambio | |
| Esquema SQLite + migraciones | |
| **Importación y exportación de catálogo por CSV** |
| **Pago mixto (efectivo + tarjeta) desde la caja** |
| **Reimpresión de tickets por folio** |
| **Reportes por cajero y exportación de ventas** |
| **Respaldo verificado de la base (VACUUM INTO)** |
| **Categorías, edición de productos y alerta de stock** |
| Inventario con libro de movimientos (entradas, mermas) | |
| Ticket ESC/POS (58 y 80 mm) | |
| Login por PIN con roles y bloqueo | |
| Apertura de caja con fondo | |
| Retiros, entradas y gastos con motivo | |
| Corte de caja con arqueo por denominaciones | |
| **Descuento, fiado y precio a mano (quedan anotados; el PIN es sólo para entrar)** | |
| **Instaladores 0.6.2 (.deb, AppImage, .exe de Windows)** | |
| **Facturación CFDI 4.0 por carpetas (global, nota, complemento de pagos)** | |
| **Clientes, límite de crédito y abonos (cartera)** | |
| **`pos-api`: un solo trait, interfaz intercambiable** | |
| **`pos-server`: la misma API por la LAN (JSON/TCP)** | |
| Pantallas de venta, catálogo, cartera, reportes y config | |
| **Auditoría visible (F6): quién hizo qué y quién autorizó** | |
| **Conteo físico asistido (F5) con ajuste firmado** | |
| **Mapeo a mano de las columnas del CSV** | |
| **Actualizaciones firmadas con respaldo y marcha atrás** | |

**270 pruebas automáticas en verde** (incluidas el día completo de operación
por HTTP y la importación de 10,000 renglones en 91 ms).

---

## Cómo ejecutarlo

```bash
# 1. Instala Rust (si no lo tienes): https://rustup.rs
# 2. Compila y corre (la caja es el programa por omisión):
cd pos-rust
cargo run --release

# Sin tocar ninguna impresora (útil en desarrollo):
cargo run --release -- --dry-run

# Base de datos en otro lugar:
cargo run --release -- --db /ruta/a/mi-tienda.db

# Sin interfaz: deja la API escuchando para la app movil (LAN)
cargo run --release -- --servir --puerto 8080

# Esta PC como terminal de la caja que ya esta corriendo alla
cargo run --release -- --api http://192.168.1.50:8080
```

### En Windows (nativo, sin Linux)

Doble clic a `scripts\iniciar.bat` —o en PowerShell:

```powershell
.\scripts\iniciar.ps1
# o a mano:
cargo build --release -p pos-app --bin pos
.\target\release\pos.exe
.\target\release\pos.exe --checar        # prueba sin ventana
.\target\release\pos.exe --probar-login  # entra, busca y sale
.\packaging\construir-windows.ps1        # pos.exe + instalador NSIS
```

Notas Windows:

- La base vive en `%LOCALAPPDATA%\PuntoDeVenta\pos.db` (con espacios y
  acentos en la ruta no pasa nada) y los errores en
  `%LOCALAPPDATA%\PuntoDeVenta\error.log`.
- Si la ventana no abre y el log dice OpenGL/WGL/context:
  `set POS_SOFTWARE_GL=1 && pos.exe` (video viejo, modo software).
- Sin ventana igual vendes desde el celular: `pos.exe --servir`.

Para probar la importación del catálogo hay un archivo de ejemplo con 25
productos de abarrotes en `ejemplos/catalogo_ejemplo.csv`: pégale la ruta en
Catálogo → *Importar catálogo desde CSV*, o arrastra el archivo a la ventana.

El cobro acepta **varios pagos a la vez** (efectivo + tarjeta, por ejemplo): en
la ventana de cobro agrega un renglón por cada método y el sistema dice cuánto
falta y cuánto cambio dar.

La primera vez:

1. Siembra un **catálogo demo de 10 productos de abarrotes** (bórralos desde
   Catálogo).
2. Crea el usuario **`admin` con PIN `1234`** si la base está vacía.
3. Al entrar te pide **abrir caja** con el fondo de cambio.

El día completo es: *abrir caja → vender → retirar/gastar → arquear (F9) →
cerrar*. Ninguna venta se guarda sin caja abierta: así el corte nunca miente.
La base queda en `~/.local/share/punto-de-venta/data.db`.

### Atajos

| Tecla | Acción |
|-------|--------|
| `F2` | Venta / enfocar búsqueda |
| `F3` | Catálogo |
| `F4` | Cobrar (abre el diálogo y confirma) |
| `F5` | Conteo físico |
| `F6` | Auditoría |
| `F7` | Clientes y cartera |
| `F8` | Reportes |
| `F9` | Caja (corte, retiros, arqueo) |
| `F10` | Configuración |
| `Supr` | Borrar el último producto del ticket |
| `Esc` | Cancelar / volver |

El lector de código de barras funciona como teclado: escanea y el código entra
en el campo de búsqueda; `Enter` lo agrega al ticket.

---

## Estructura

```
pos-rust/
├── crates/
│   ├── pos-core/   Dominio puro: Money (centavos), Quantity (milésimos),
│   │               impuestos, caja y la venta. Cero dependencias, cero I/O.
│   ├── pos-api/    EL CONTRATO: trait PosApi + Comando + DTOs.
│   │               Toda la orquestación de negocio vive aquí (LocalApi),
│   │               incluido el lector/escritor de CSV (importar.rs).
│   ├── pos-server/ Servidor de la LAN: HTTP/JSON sobre std::net, sin tokio.
│   ├── pos-db/     SQLite (rusqlite): migraciones versionadas, productos,
│   │               ventas, inventario, cartera, reportes.
│   ├── pos-hw/     Tickets ESC/POS y el driver universal de térmicas:
│   │               USB, serie (termios), red y Windows (spooler RAW).
│   ├── pos-ui/     Interfaz egui: tema, pantallas y atajos.
│   └── pos-app/    El ejecutable (`pos`).
├── migrations/     SQL versionado (0001_init … 0003_cartera)
├── ejemplos/       cliente_lan.py: cómo habla una app móvil con el POS
└── Cargo.toml      Perfil release optimizado a tamaño (LTO + strip)
```

**Regla de oro:** `pos-core` no sabe que existen SQLite ni las impresoras, y
`pos-ui` no sabe que existe SQLite. Todo cruza por tipos puros o por la API, así
que las reglas de negocio se prueban en microsegundos y sin hardware.

---

## Desacoplado de verdad: la interfaz es una piel

El requisito que ordena todo el diseño es este: **cambiar el motor de interfaz
(o la interfaz completa) no debe romper nada**, y la misma API debe servirle a
una app móvil por la LAN. Se resuelve con un solo trait:

```text
   ┌───────────────┐   ┌────────────────┐   ┌──────────────┐
   │  UI escritorio│   │  App móvil LAN │   │  CLI / pruebas│
   └───────┬───────┘   └────────┬───────┘   └──────┬───────┘
           │                    │                  │
           └────────────────────┴──────────────────┘
                              │
                    trait PosApi  (un solo contrato)
                              │
              ┌───────────────┴────────────────┐
              │                                │
       LocalApi (en proceso)          HttpApi (JSON/TCP)
              │                                │
              ▼                                ▼
        pos-db + pos-core              pos-server → pos-db + pos-core
```

- `pos-ui` **no importa `pos-db`**: no podría hacer una consulta aunque quisiera.
  Si mañana cambiamos egui por iced, Slint o un WebView, se reescriben las
  pantallas y nada más.
- Las reglas difíciles (no vender sin caja, no descontar sin supervisor, no fiar
  más del límite) viven en `pos-api`, así que **la app móvil no puede
  saltárselas**: las consulta por HTTP y recibe el mismo "no".
- Los 18 comandos son un enum cerrado y serializable. Ese enum es la documentación
  de la API.

### Probarlo en dos terminales

```bash
# Terminal 1 — la caja, sin interfaz, atendiendo la LAN:
cargo run --release -- --servir --puerto 8080
#   Sirviendo el punto de venta en el puerto 8080.
#   App móvil: apunta a http://192.168.1.50:8080

# Terminal 2 — cualquier cliente HTTP. Éste es Python, sin librerías:
python3 ejemplos/cliente_lan.py 192.168.1.50 8080
```

`ejemplos/cliente_lan.py` recorre el día completo (login → abrir caja → vender →
descuento con PIN → fiado → abono → corte) y es la referencia para quien escriba
el cliente Android o iOS. No hay dependencias: `POST /api` con
`{"token": ..., "comando": ...}` y la respuesta es `{"Ok": ...}` o
`{"Error": "..."}` con el mensaje listo para mostrarse.

---

## Decisiones técnicas que importan

1. **Nada de `f64` para dinero.** Todo son centavos (`i64`). El IVA se separa
   con redondeo *half away from zero*, que es el criterio del SAT.
2. **Los totales se calculan una sola vez.** `Sale::tax_breakdown()` reparte el
   descuento global entre líneas (el último renglón absorbe el residuo) y de ahí
   salen el ticket, la base de datos y el reporte. Imposible que cuadren distinto.
3. **Una venta = una transacción.** Se escriben venta, líneas, pagos,
   movimientos de inventario y la cola `outbox` juntos. Si se va la luz a mitad
   del cobro, la venta existe completa o no existe.
4. **Escáner HID.** El lector se conecta como teclado: cero drivers.
5. **El ticket son bytes.** `pos-hw` fabrica el arreglo ESC/POS, así que se
   prueba comparandolo contra lo esperado, sin impresora conectada.
6. **El efectivo esperado se deriva, no se acumula.** El corte suma fondo +
   ventas en efectivo + entradas − retiros − gastos, consultando los hechos.
   No hay un contador que se pueda desincronizar.
7. **El PIN nunca se guarda en claro.** Argon2id con sal aleatoria, más bloqueo
   tras 5 intentos fallidos. Cada acción sensible queda en `audit_log`.
8. **La interfaz no tiene permiso de hacer reglas.** Sólo dibuja y llama a
   `PosApi`. Por eso cambiar de motor gráfico es cosmético y por eso un cliente
   nuevo (móvil) hereda las mismas validaciones sin reescribirlas.
9. **Sin `tokio`, sin `axum`.** El servidor de la LAN es `std::net` con un hilo
   por conexión: en un mostrador hay tres dispositivos y cada mega de RAM vale
   más que la escala.
10. **La importación es todo o nada y dice el renglón.** Si un renglón del CSV
    está mal, no se escribe nada y el reporte dice `renglón 47: no entiendo el
    precio 'caro'`. Nunca un catálogo a medias.
11. **El stock no se edita, se mueve.** El campo de existencias se cambia con
    entradas, ajustes y mermas con motivo; así el libro de movimientos siempre
    explica por qué hay lo que hay.

---

## Verificación

```bash
cargo test --workspace                        # 324 pruebas
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo build --release                         # binario en target/release/pos
```

Cobertura por crate:
- `pos-core` (33): formato de montos, redondeo, IVA incluido/agregado,
  descuento global, pagos mixtos, cambio, utilidad, roles y permisos, arqueo por
  denominaciones, corte con faltante/sobrante.
- `pos-db` (45): migraciones idempotentes, búsqueda por código y por nombre,
  inventario contra libro de movimientos, atomicidad de la venta, folios únicos,
  cancelación con reposición de stock, totales del día, integridad tras "apagón",
  PIN con Argon2 y bloqueo por intentos, sesiones de caja, retiros/gastos y
  cierre con diferencia, clientes, límite de crédito, fiado y abonos,
  **edición de productos, importación masiva transaccional, categorías,
  ventas por cajero y respaldo verificado**.
- `pos-api` (48): login y token, sesión vencida, bloqueo por 5 intentos, no
  vender sin caja, ciclo completo del día, arqueo descuadrado, descuento sin
  supervisor, precio manual, fiado con límite, inventario descontado,
  **importación y exportación CSV** (14 pruebas del lector: encabezados en
  español/inglés, `precio` vs `precio_costo`, `$18.50` / `18,50` / `1,234.56`,
  10,000 renglones, ida y vuelta exportar→importar), **pago mixto**, precio
  manual, reimpresión por folio, ventas por cajero, categorías y respaldo.
- `pos-server` (3): arranque y salud, **el día completo por HTTP**, dos
  dispositivos viendo el mismo estado.
- `pos-hw` (12): bytes del ticket, acentos y ñ a CP437, QR, EAN-13, pulso de
  cajón, ancho de 58 mm, vista previa.
- `pos-ui` (3): fecha local sin dependencias (algoritmo civil de Hinnant) y
  el formateo de cantidades del conteo (12.5 piezas, -4 de merma).
- `pos-update` (27): firma y verificación Ed25519, hash, versiones (ni atrás ni
  saltos), respaldo del ejecutable, cambio atómico, prueba de arranque, marcha
  atrás automática, revertir a mano y la prueba que falla a propósito.

---

## Automatización y pruebas sin adivinar

Hay dos caminos, y elegir el correcto ahorra horas:

```powershell
# 1. Lógica y flujos completos, sin ventana: la misma API que usa la interfaz
.\target\debug\pos.exe --db "$env:TEMP\prueba.db" --guion .\ejemplos\guion-venta.json
.\scripts\headless-smoke.ps1 -Build

# 2. Interfaz real: ventana, clics y capturas con PrintWindow
.\scripts\demo-venta-ui.ps1 -Build
```

El **modo headless** tiene dos formas: `--headless` deja la API HTTP escuchando
para la LAN y `--guion RUTA.json` ejecuta una lista de comandos y sale con un
reporte. El token del login se guarda solo. Los guiones usan dinero en centavos
y cantidades en milésimos, y conviene correrlos contra una base temporal.

La **automatización de la interfaz** está en `scripts/pos-ui.psm1`. La regla
que evita clics desplazados es:

```
pantalla = (GetWindowRect.Left + x_imagen, GetWindowRect.Top + y_imagen)
```

`PrintWindow` captura la ventana con marco, igual que `GetWindowRect`; no se
suma `ClientToScreen`. El módulo sabe iniciar la app, entrar con PIN, buscar
productos, navegar con F2…F10, redimensionar el cliente y capturar.

Guía completa, formato de los guiones, catálogo de comandos y trampas:
**[`docs/automatizacion.md`](docs/automatizacion.md)**. Índice de scripts:
**[`scripts/README.md`](scripts/README.md)**. Instrucciones para agentes:
**[`AGENTS.md`](AGENTS.md)**.

---

## Rendimiento objetivo (hardware real de la tienda)

| Métrica | Objetivo |
|---------|----------|
| Arranque en frío | < 2 s en Celeron + HDD |
| RAM residente | < 90 MB con 50 000 SKUs |
| Tamaño del binario | 7.2 MB (medido) |
| Búsqueda con 2 caracteres | < 50 ms |
| CPU en reposo | 0 % (egui solo repinta al interactuar) |

Mide en el equipo destino antes de optimizar: `cargo run --release` y observa
el consumo real. Si el Celeron no trae OpenGL 3.3, arranca con
`LIBGL_ALWAYS_SOFTWARE=1` o compila la variante con backend `wgpu`.

---

## Respaldo automático

Se hace solo: **al arrancar, cada 15 minutos si ya toca, y al cortar la caja** (el mejor momento del día).
Corre en un hilo aparte, así que un respaldo grande no congela la venta.

- Copia con `VACUUM INTO`, **verificada antes de darla por buena**: si la copia no pasa el
  `integrity_check`, se descarta y se reporta como error, no como "hecho".
- **Rotación**: se conservan los últimos N (14 por defecto) y se borran los viejos. Nunca deja la
  carpeta vacía, aunque le pidas conservar 0.
- Carpeta configurable; detecta las memorias USB montadas y las ofrece en la pantalla.
- **El corte de caja nunca se pierde** aunque el respaldo falle (la USB desconectada no puede
  arruinar el cierre del día).

```bash
pos --respaldar                 # respaldo de un golpe (útil para una tarea programada)
pos --restaurar respaldo-....db # recupera: lo verifica antes y deja una copia de lo que había
```

Un respaldo dañado **no se restaura**: `--restaurar` lo detecta, avisa y no toca tu base.

Se configura desde Configuración (F10) o por la API: `Respaldos`, `RespaldarAhora`,
`VerificarRespaldos` y `Unidades`.

## Auditoría (F6)

La bitácora de quién hizo qué y **quién autorizó qué**. Se consulta cuando falta dinero.

- **Nunca se borra** y **no la ve cualquiera**: permiso `VerAuditoria` (encargado y administrador; los
  cajeros no).
- Cada autorización se anota con el detalle, no con un "ok":
  `El Jefe autorizó cambiar el precio a mano: precio a mano en 7503: $27.00 → $15.00`
- Cada venta queda con folio, productos y forma de pago: `Folio 000002 · 7503 x1 · $15.00 · efectivo`
  (se puede buscar por producto).
- También: devoluciones, cortes que no cuadran (con la diferencia), PIN equivocado y bloqueo,
  respaldos, importaciones y cambios de configuración.
- Filtros por fecha, usuario, acción y texto libre; atajo "Solo autorizaciones".

Comando del API: `Auditoria` (los mismos datos para la app móvil).

## Conteo físico (F5) y mapeo de columnas

Dos cosas que se piden siempre en una tienda real.

**Conteo físico asistido.** Cuando el sistema dice 12 y en el anaquel hay 9.

- Se escanea y se escribe cuántas hay. Volver a escanear **reemplaza**, no suma.
- La diferencia se calcula contra **el stock de ese momento**, no contra la foto
  del principio: se puede vender mientras se cuenta sin corromper el ajuste.
- Lo que nadie contó **no se ajusta a cero**. Nunca.
- El resumen dice cuántos productos, cuántos con diferencia, cuánto faltó,
  cuánto sobró y la merma **valuada a costo** (que es la que duele).
- Dos cierres: *cerrar sin aplicar* (ves el reporte, no se toca nada) y *cerrar
  y aplicar*, que mueve el inventario y **lo firma el encargado** (permiso
  `AjusteInventario`: administrador o supervisor; un cajero no).
- Filtros "por contar" y "con diferencia", para no volver a leer lo que ya
  cuadra, y alcance por **categoría** (hoy los abarrotes, mañana la botana).
- Se puede cancelar sin dejar rastro ni tocar el inventario.

Comandos del API: `IniciarConteo`, `VerConteo`, `ContarProducto`,
`CerrarConteo`, `CancelarConteo` (los mismos datos para la app móvil).

**Mapeo a mano de las columnas del CSV.** El importador ya adivina
(`descripcion`, `precio`, `costo`, `iva`, `existencia`, `codigo de barras`…),
pero los archivos de los proveedores vienen chuecos. Con *Elegir columnas…* el
sistema lee el archivo, muestra el encabezado **con tres renglones de ejemplo
al lado** y deja decir, columna por columna, qué es cada una —o que se ignore—.
Las categorías del CSV se crean al vuelo.

Comandos del API: `AnalizarCsv` (el análisis y la lista de campos) e
`ImportarCsv` con `mapeo` (una clave por columna: `nombre`, `precio`,
`categoria`, `ignorar`…).

## Impresora térmica: un solo driver para (casi) todas

No se instala nada: ni driver del fabricante, ni CUPS, ni libusb. El programa
habla ESC/POS directo por donde salga el cable.

| Cómo está conectada | Ruta que se escribe | Qué hace el programa |
|---|---|---|
| USB (clase impresora) | `/dev/usb/lp0` | Escribe los bytes tal cual |
| USB-serial (CH340, FTDI, CP210x, Prolific) | `/dev/ttyUSB0` | Configura `termios`: 8N1 sin control de flujo, a los baudios que digas |
| Red / Wi-Fi | `192.168.1.50:9100` | TCP en crudo al puerto 9100 |
| Windows | `POS-80` | Spooler en modo **RAW** |

```bash
pos --impresoras                       # qué hay conectado y qué es
pos --probar-impresion /dev/usb/lp0    # manda el ticket de prueba
pos --probar-impresion RUTA --cajon    # y abre el cajón
```

Lo que de verdad cambia entre una térmica y otra son seis cosas, y están en un
**perfil** que se elige en Configuración: ancho (58/80 mm), página de códigos,
comando de corte (incluidas las que no tienen cuchilla), pin del cajón,
velocidad del puerto serie y si trae QR.

Sobre los acentos, un dato que salió de las tablas oficiales de Unicode y que
explica el diseño: **CP437 —la página que traen de fábrica casi todas las
térmicas chinas— no tiene `Á Í Ó Ú`**. CP850 sí cubre el español completo, así
que es la de omisión; y si una página no tiene un carácter, se le quita el
acento antes que mandar basura. Las tablas se generaron de `CP437.TXT` y
`CP850.TXT` de unicode.org, no están escritas a mano.

Las 61 pruebas de `pos-hw` se corren **sin impresora**: una configura un puerto
serie de verdad sobre una pseudoterminal (`openpty` + `termios`), otra manda un
ticket por un socket real, y la detección se prueba contra un árbol falso de
`/dev` y `/sys`. Guía completa: **[docs/guia-impresoras.md](docs/guia-impresoras.md)**.

## Actualizaciones firmadas (con marcha atrás)

El miedo no es actualizar: es actualizar y que la tienda se quede sin sistema un
sábado en la noche. Por eso el orden es: **guardar lo que funciona, probar lo
nuevo, y sólo entonces cambiar**.

- El paquete `.posup` va **firmado con Ed25519**. La llave pública va dentro del
  binario; la privada nunca sale de la máquina donde se firma. Un paquete de
  otro, alterado, truncado o reetiquetado se rechaza **antes de tocar nada** (y
  la pantalla dice por qué).
- No se baja de versión, y un paquete puede exigir una versión mínima.
- Al aplicar: se guarda el ejecutable actual (`pos.anterior`), se escribe el
  nuevo aparte, se cambia de un golpe (`rename`, atómico) y **se corre el nuevo**
  con `--checar`. Si no arranca o se cuelga, **se deshace solo** y queda el que
  sí funcionaba.
- Queda un marcador mientras la actualización no se confirma: si se corta la luz
  a media operación, al volver se sabe.
- Botón *Deshacer la última actualización* (y `pos --revertir`) para volver
  atrás a mano.

Cómo se usa:

```bash
# quien suelta la versión (una sola vez genera las llaves):
cargo run -p pos-update --bin pos-firmar --features firmar -- --generar
cargo run -p pos-update --bin pos-firmar -- --firmar target/release/pos \
    --version 0.5.0 --notas "Arregla el corte de caja" --salida 0.5.0.posup

# en la tienda, desde la terminal (o desde Configuración → Actualizaciones):
pos --actualizar 0.5.0.posup          # revisa, aplica, prueba y reporta
pos --revertir                        # vuelve al programa anterior
pos --version                         # la versión instalada
```

Comandos del API: `EstadoActualizacion`, `RevisarActualizacion`,
`AplicarActualizacion`, `RevertirActualizacion` (permiso `Configuración`: un
cajero no puede cambiar el programa).

> La llave que va en `crates/pos-update/llaves/` es la **de desarrollo**, y está
> ahí a propósito para que las pruebas firmen como firmaría quien suelta una
> versión. Para la tienda real: genera otra, recompila y guarda la privada donde
> nadie más la vea.

## Instaladores

```bash
./packaging/construir.sh            # todo lo que se puede armar en esta máquina
./packaging/construir.sh deb        # sólo el .deb
./packaging/construir.sh appimage   # AppImage (o AppDir + tar.gz si no hay appimagetool)
./packaging/construir.sh windows    # pos.exe + instalador NSIS
```

| Salida                                              | Qué es                                                                 |
| --------------------------------------------------- | ---------------------------------------------------------------------- |
| `punto-de-venta_0.6.2_amd64.deb`                    | Paquete Debian/Ubuntu/Mint. Binario en `/usr/bin/pos`, menú y regla udev de la impresora. Los datos viven en `~/.local/share/punto-de-venta/` y **el desinstalador nunca los borra**. |
| `PuntoDeVenta-0.6.2-x86_64.AppImage`                | Un archivo: se marca ejecutable y corre. Lleva el manual y `cliente_movil.py`. |
| `PuntoDeVenta-0.6.2-Instalador.exe`                 | Instalador de Windows (NSIS, en español), cruzado desde Linux con mingw. |
| `PuntoDeVenta.AppDir/` + `.tar.gz` portable         | Respaldo si no hay `appimagetool`. Se descomprime y corre sin instalar. |

## Galería

Capturas reales de cada pantalla (tema Día, 1366×768): **[`docs/galeria.html`](docs/galeria.html)**.
Se regenera con `python3 scripts/armar_galeria.py` a partir de `crates/pos-ui/tests/snapshots/`.

## Prueba de apagones

`kill -9` en el peor momento: se manda una venta y se mata el proceso mientras la está escribiendo.

```bash
cargo test --test apagon                       # 40 apagones
POS_ITERACIONES=200 cargo test --test apagon   # la corrida completa
```

Última corrida: **200 apagones en 45.3 s**, 1,100 intentos de venta, **0 inconsistencias**.
De las 200 ventas interrumpidas, 188 se alcanzaron a guardar completas y 12 se descartaron enteras
(nunca a medias). Tras cada apagón se reabre la base y se cobra otra venta para comprobar que sigue
sirviendo. Se verifica: `integrity_check`, que ninguna venta quede sin renglones, que los pagos sumen
el total, que el neto cuadre con el IVA, que el inventario cuadre con sus movimientos y que no haya
renglones ni pagos huérfanos.

## Manual del cajero

`docs/Manual-del-cajero.pdf` — dos hojas A4 para imprimirlas y pegarlas en la caja (versión **0.6.2**: tres roles, LAN del celular, el cajero corta caja y da de alta productos).
Fuente editable: `docs/manual-cajero.html` (se regenera con WeasyPrint: `python3 -c "from weasyprint import HTML; HTML('docs/manual-cajero.html').render().write_pdf('docs/Manual-del-cajero.pdf')"`).
