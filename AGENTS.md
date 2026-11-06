# AGENTS.md — guía para quien trabaje en este repositorio

Este archivo es para agentes y personas que llegan nuevos. Resume cómo está
organizado el POS, cómo se compila, cómo se prueba **sin adivinar** y qué
reglas no se rompen.

## 1. Qué es

POS de mostrador para retail en Rust + egui (OpenGL), pensado para equipos
viejos. Un solo ejecutable, SQLite embebida, ticket ESC/POS directo, offline.

- `crates/pos-core`: dominio puro (dinero en centavos, cantidades en milésimos,
  IVA, venta, caja, roles).
- `crates/pos-db`: SQLite, migraciones, consultas, transacciones.
- `crates/pos-api`: el contrato (`Comando`, `Cuerpo`, `PosApi`) y `LocalApi`.
- `crates/pos-server`: HTTP mínimo para la LAN (`POST /api`).
- `crates/pos-hw`: impresora térmica, ESC/POS, EAN-13, cajón.
- `crates/pos-ui`: **sólo interfaz**; no importa `pos-db` ni reglas de negocio.
- `crates/pos-app`: ejecutable, CLI, siembra, modos de arranque, guiones.
- `crates/pos-update`: actualizaciones firmadas.
- `crates/pos-fiscal`: CFDI 4.0.

## 2. Compilar y correr

```powershell
cargo build -p pos-app                 # debug: target\debug\pos.exe
cargo run -p pos-app                   # abre la ventana
cargo check -p pos-app
cargo test --workspace
```

La base real vive en `%LOCALAPPDATA%\PuntoDeVenta\pos.db`; los errores en
`%LOCALAPPDATA%\PuntoDeVenta\error.log`. Para pruebas usa siempre `--db` con
una ruta temporal.

Modos:

```powershell
pos.exe                                   # ventana + API local
pos.exe --db C:\temp\prueba.db            # ventana contra otra base
pos.exe --headless --puerto 8080          # API LAN, sin ventana
pos.exe --guion ruta.json --db C:\temp\prueba.db   # ejecuta y sale
pos.exe --dry-run                         # sin impresora
```

## 3. Reglas del proyecto

- **Todo en español**: nombres de pantalla, textos, mensajes de error y
  comentarios. Los identificadores del dominio también (`venta`, `caja`).
- **Dinero en centavos (`i64`)** y cantidades en milésimos. No uses `f64` para
  dinero.
- **La UI no tiene reglas de negocio**: llama a `PosApi`. Si necesitas calcular
  algo, va en `pos-core` o `pos-api`.
- **No rompas la funcionalidad existente**: venta, ticket, descuentos, clientes,
  cobro mixto, caja, conteo, reportes, auditoría y configuración.
- **No borres archivos que no entiendas**: el repo no está en git; un archivo
  raro puede ser trabajo del usuario.
- **No agregues dependencias** si la biblioteca estándar o lo que ya hay
  alcanza. El proyecto presume de binario y RAM pequeños.

## 4. Diseño de la interfaz (D2)

El rediseño aprobado es la mezcla **B + C refinada**:

- Fondo off-white `#F5F5F0`; paneles y tarjetas blancos.
- Sidebar oscura verde sage (`#5C7A63` como acento), con etiquetas.
- A ≤1180 px de ancho, la sidebar se colapsa a un **rail de 64 px**.
- Lima `#BEE36A` para **Cobrar**, navegación activa y selección; texto oscuro
  encima.
- Catálogo: tabla densa con monogramas, precio/existencia alineados a la
  derecha y botón “Agregar”.
- Filas de **54 px** (53 en rail). Objetivo: 8 productos visibles a 1366×768 y
  6 a 1024×600.
- Ticket: compacto, sin métodos de pago; sólo total y **Cobrar**. Scroll
  siempre visible y barra de feedback con “Deshacer”.
- Nada de texto cortado, controles fuera de pantalla ni solapamientos.

Paleta y helpers en `crates/pos-ui/src/theme.rs` (`fondo`, `tarjeta`, `lima`,
`lima_tenue`, `texto`, `texto_suave`, `borde`). No escribas colores sueltos en
`app.rs`; usa el tema.

## 5. Probar la interfaz sin adivinar

Lee **[`docs/automatizacion.md`](docs/automatizacion.md)** antes de tocar
coordenadas. Lo esencial:

```powershell
Import-Module .\scripts\pos-ui.psm1 -Force
Stop-PosUi -All -Force
Start-PosUi -Build -ClientWidth 1280 -ClientHeight 800
Enter-PosUiLogin
Add-PosUiProduct -Search 'Aceite'
Save-PosUiCapture -Path .\capturas\automatizacion\venta.png
Stop-PosUi
```

O el demo completo con base limpia y caja abierta:

```powershell
.\scripts\demo-venta-ui.ps1 -Build
```

Regla de coordenadas:

```
pantalla = (GetWindowRect.Left + x_imagen, GetWindowRect.Top + y_imagen)
```

`PrintWindow` captura la ventana **con marco**, igual que `GetWindowRect`. No
uses `ClientToScreen` ni `CopyFromScreen`.

### Capturas que hay que revisar

- 1280×800 (base del módulo).
- 1366×768 (equipo de mostrador típico).
- 1024×600 (equipo viejo / ventana pequeña).
- 1920×1080 (pantalla grande).
- Táctil: botones grandes, sin hover como única pista.

Después de cada cambio de UI: compila, captura y **mira la imagen**. No declares
una pantalla terminada sin captura.

## 6. Probar la lógica sin ventana

```powershell
.\scripts\headless-smoke.ps1 -Build
.\target\debug\pos.exe --db "$env:TEMP\guion.db" --guion .\ejemplos\guion-venta.json
```

- `--headless` levanta `POST /api` para la LAN.
- `--guion` ejecuta `pos_api::Comando` desde JSON y guarda un reporte.
- El token del `Login` se guarda solo.
- Usa base temporal: los guiones cobran y cierran caja.
- `ejemplos/guion-venta.json` es el ejemplo completo; `guion-preparar-caja.json`
  deja una caja abierta para el demo de UI.

## 7. Trampas conocidas

| Trampa | Qué hacer |
| --- | --- |
| `pos.exe` bloquea `cargo build` | `Stop-PosUi -All -Force` antes de compilar |
| `POS_SOFTWARE_GL=1` falla con `egui_glow requires opengl 2.0+` | No lo uses en este equipo; actualiza el driver o usa una sesión con GPU |
| Ventana sin título / negra | OpenGL falló; mira `error.log` |
| Clic desplazado | Súmale el origen de `GetWindowRect`, no el del cliente |
| `-Db` en PowerShell | Usa `-Database` (alias `Db` → `Debug`) |
| Redimensionar al instante rompe el renderer | `Start-PosUi` espera ~2 s; no lo quites |
| PIN incorrecto mueve el teclado ~14 px | Vuelve a capturar y recalcula |
| `PrintWindow` en servicio/remoto | Necesita sesión de escritorio real |
| Captura de 1280×800 mide ~1296×839 | Es correcto: incluye marco; las coordenadas también |

## 8. Dónde está cada pantalla

Casi todo vive en `crates/pos-ui/src/app.rs`:

- `PosApp` y `Pantalla` (estado y navegación).
- `pantalla_venta` / `panel_ticket` (Venta).
- `pantalla_catalogo`, `pantalla_caja`, `pantalla_clientes`,
  `pantalla_reportes`, `pantalla_conteo`, `pantalla_auditoria`,
  `pantalla_config`.
- `pedidos.rs`: `pantalla_pedidos` (módulo aparte).
- Helpers de tabla, badges, botones y formularios alrededor de cada pantalla.
- `theme.rs`: paleta, tipografía, estilos de botones.
- `iconos.rs`: iconos vectoriales; no metas PNGs.

Si agregas una pantalla: añádela a `Pantalla`, al sidebar, a los atajos
(`F2`…`F10`), a `Open-PosUiScreen` del módulo y a esta lista.

## 9. Definición de “terminado” para un cambio de UI

1. `cargo build -p pos-app` sin errores.
2. `cargo check -p pos-app` sin errores nuevos.
3. Flujo headless sigue pasando (`headless-smoke.ps1`).
4. Captura real a 1280×800, 1366×768 y 1024×600 (y 1920×1080 si aplica).
5. Sin texto cortado, sin solapamientos, sin botones fuera de pantalla.
6. La funcionalidad de venta/ticket/cobro/caja sigue igual.
7. Documentación actualizada si cambió un comando, un script o un atajo.

## 10. Documentos útiles

- `README.md`: producto, instalación, atajos, verificación.
- `docs/automatizacion.md`: guiones, API HTTP, módulo de UI y trampas.
- `scripts/README.md`: todos los scripts y sus parámetros.
- `docs/manual-cajero.html` y `docs/Manual-del-cajero.pdf`: uso diario.
- `docs/facturacion-cfdi.md`, `docs/guia-impresoras.md`,
  `docs/rendimiento.md`, `docs/firma-windows.md`.
