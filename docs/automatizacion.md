# Automatización y pruebas

Esta guía existe para que nadie vuelva a perder horas adivinando cómo se abre
la ventana, dónde está un botón o cómo se prueba una venta sin dar clics. Hay
dos caminos y conviene elegir el correcto antes de empezar:

| Camino | Cuándo usarlo | Qué prueba |
| --- | --- | --- |
| **Headless** (`--guion` / `--headless`) | Lógica, API, flujos completos, datos | La misma `LocalApi` que usa la interfaz, sin ventana |
| **UI real** (`scripts/pos-ui.psm1`) | Aspecto, geometría, textos, capturas | La ventana de egui/OpenGL y lo que ve el cajero |

La regla de oro: **la lógica se prueba headless; la interfaz se prueba con la
ventana real y capturas**. No se automatizan clics para probar reglas de negocio
si un guion puede hacerlo en milisegundos.

---

## 1. Modo headless

El ejecutable tiene dos formas sin ventana, y no son lo mismo:

```powershell
# Servidor HTTP para la LAN (se queda escuchando)
.\target\debug\pos.exe --headless --puerto 8080 --db C:\temp\tienda.db

# Ejecutor de un guion JSON (hace los pasos y termina)
.\target\debug\pos.exe --guion .\ejemplos\guion-venta.json --db C:\temp\tienda.db
```

`--servir` es un alias de `--headless`. `--guion` no abre puerto: abre la base,
ejecuta los comandos y sale con código `0` si todo salió bien.

### 1.1 El contrato HTTP

El servidor habla un contrato mínimo, sin tokio ni frameworks:

```http
POST /api
Content-Type: application/json

{ "token": "opcional", "comando": { "Login": { "username": "admin", "pin": "1234" } } }
```

La respuesta es un enum cerrado:

```json
{ "Ok": { "Sesion": { "usuario": { "display_name": "Administrador" }, "token": "..." } } }
```

o

```json
{ "Error": "texto para el cajero" }
```

`GET /api/salud` responde `{"estado":"ok"}` y sirve para esperar a que el
servidor arranque. El servidor siempre contesta HTTP 200; el error de negocio
va dentro del JSON. Por eso los scripts miran `respuesta.Ok` / `respuesta.Error`
y no el código HTTP.

Los comandos son exactamente `pos_api::Comando`. La lista completa, con los
campos de cada variante, está en `crates/pos-api/src/api.rs`. El JSON usa la
representación por defecto de serde:

- variante sin datos: `"EstadoCaja"`
- variante con datos: `{ "Login": { "username": "admin", "pin": "1234" } }`
- los campos van en `snake_case`
- **todo el dinero va en centavos** (`4200` = $42.00)
- **las cantidades van en milésimos** (`1000` = 1 pieza, `1500` = 1.5)

### 1.2 Formato de un guion

```json
{
  "nombre": "Venta de humo",
  "reporte": "capturas/automatizacion/guion-venta-reporte.json",
  "detener_al_fallar": true,
  "json": false,
  "pasos": [
    {
      "descripcion": "Entrar como admin",
      "comando": { "Login": { "username": "admin", "pin": "1234" } }
    },
    {
      "descripcion": "Abrir caja con $100",
      "comando": { "AbrirCaja": { "fondo_cents": 10000 } }
    },
    {
      "descripcion": "Salir",
      "comando": "Salir"
    }
  ]
}
```

| Campo | Obligatorio | Qué hace |
| --- | --- | --- |
| `nombre` | no | Título del reporte y de la salida por consola |
| `reporte` | no | Ruta donde se escribe el JSON con el resultado de cada paso |
| `detener_al_fallar` | no | `true` por omisión: se detiene en el primer fallo |
| `json` | no | `true` imprime sólo el reporte JSON (útil para otra herramienta) |
| `pasos` | **sí** | Arreglo con al menos un paso |
| `pasos[].descripcion` | no | Texto legible; si falta se usa `paso N` |
| `pasos[].comando` | **sí** | Una variante de `pos_api::Comando` |
| `pasos[].esperar` | no | `"ok"` (omisión), `"error"` o `"cualquiera"` |

El token de la sesión se guarda automáticamente después de un `Login` y se
manda en los pasos siguientes. Un `Salir` lo borra. No hay que copiarlo a mano.

Cada paso del reporte incluye `ok`, `duracion_ms` y la respuesta completa, así
que un fallo se puede diagnosticar sin volver a correr el guion.

### 1.3 Guion de ejemplo

`ejemplos/guion-venta.json` hace el día completo:

1. Login de `admin`.
2. Consulta el estado de la caja.
3. Abre caja con $100.
4. Busca “Aceite”.
5. Cobra una pieza con $50 recibidos.
6. Comprueba que la venta aparece en recientes.
7. Comprueba que la caja sumó el ticket.
8. Cierra la caja con autorización de admin.
9. Sale.

Se corre así:

```powershell
$db = Join-Path $env:TEMP "pos-guion-demo.db"
Remove-Item $db -Force -ErrorAction SilentlyContinue
.\target\debug\pos.exe --db $db --guion .\ejemplos\guion-venta.json
```

Salida real de la última corrida:

```
Guion: Venta de humo (headless)
Base:  C:\Users\...\pos-guion-demo.db
Pasos: 9

[1/9] Entrar como admin ... OK (27 ms)
       sesión de Administrador
...
[5/9] Cobrar una pieza de Aceite 1-2-3 1 L ... OK (1 ms)
       folio 000001 · total 4200 centavos · cambio 800 centavos
...
Resumen: 9/9 OK, 0 fallo(s), 42 ms
```

### 1.4 Comandos que más se usan

| Comando | JSON | Devuelve |
| --- | --- | --- |
| Login | `{"Login":{"username":"admin","pin":"1234"}}` | `Sesion` con token |
| Estado de caja | `"EstadoCaja"` | `Caja` (abierta, fondo, tickets, vendido) |
| Abrir caja | `{"AbrirCaja":{"fondo_cents":10000}}` | `Vacio` |
| Buscar | `{"BuscarProductos":{"termino":"Aceite","limite":5}}` | `Productos` |
| Cobrar | `{"Cobrar":{"venta":{...}}}` | `Venta` con folio, total y cambio |
| Ventas recientes | `{"VentasRecientes":{"limite":5}}` | `Ventas` |
| Cerrar caja | `{"CerrarCaja":{"piezas":[],"supervisor":{"username":"admin","pin":"1234"}}}` | `Vacio` |
| Salir | `"Salir"` | `Vacio` |

La venta de `Cobrar` tiene esta forma (los campos que no se usan van en `null`
o vacíos):

```json
{
  "lineas": [
    {
      "sku": "7507",
      "descripcion": "Aceite 1-2-3 1 L",
      "qty_millis": 1000,
      "precio_cents": 4200,
      "descuento_cents": 0
    }
  ],
  "descuento_global_cents": 0,
  "pagos": [
    { "metodo": "efectivo", "monto_cents": 4200, "recibido_cents": 5000 }
  ],
  "cliente_id": null,
  "nota": "prueba",
  "supervisor": null
}
```

### 1.5 Reglas para escribir guiones

- Usa **una base temporal** (`--db %TEMP%\algo.db`). Un guion que cobra o
  cierra caja cambia datos; no lo corras contra la base de la tienda.
- La base se siembra sola al abrirla: catálogo demo, `admin`/`1234` y
  `ana`/`4321`. No hace falta preparar nada.
- Si un paso espera un error, pon `"esperar": "error"`. Así se prueban los
  caminos tristes (vender sin caja, PIN incorrecto, etc.).
- Para depurar, corre el mismo guion con `"json": true` o lee el reporte.
- El código de salida es `0` si todos los pasos cumplieron, `1` si alguno
  falló y `2` si el archivo no existe o no es JSON válido.

### 1.6 Prueba de humo HTTP

`scripts/headless-smoke.ps1` levanta `--headless` en un puerto libre, espera
`/api/salud`, hace login, abre caja, busca, cobra, comprueba la venta y cierra
caja. No toca la base real.

```powershell
.\scripts\headless-smoke.ps1 -Build
```

Salida esperada:

```
[1] El servidor headless responde /api/salud ... OK
...
Resumen: 9/9 OK, 0 fallo(s)
Reporte: capturas\automatizacion\headless-smoke-reporte.json
```

Parámetros: `-Port`, `-Database`, `-ReportPath`, `-KeepDb`, `-Build`, `-Exe`.

---

## 2. Automatización de la interfaz real

### 2.1 La regla que ahorra horas

`PrintWindow` captura **la ventana completa**, incluido el marco y la barra de
título. `GetWindowRect` devuelve ese mismo rectángulo. Por eso:

```
pantalla = (rect.Left + x_imagen, rect.Top + y_imagen)
```

No se usa `ClientToScreen`: sumaría otra vez el borde y el título, y los clics
caerían unos 8 px a la izquierda y 28 px arriba. Ése fue el error que hizo
perder tiempo antes.

```
GetWindowRect:  (234, 234) ─────────────────────────────┐
                │  título (~28 px)                       │
                │  borde (~8 px)                         │
                │  ┌──────────────────────────────────┐  │
                │  │ cliente (1280x800)               │  │
                │  │  (x_imagen, y_imagen)            │  │
                │  └──────────────────────────────────┘  │
                └────────────────────────────────────────┘
```

Una captura de 1280×800 de cliente mide ~1296×839 con el marco. Eso es normal:
la imagen y `GetWindowRect` miden lo mismo, y por eso la fórmula funciona.

### 2.2 Uso rápido

```powershell
Import-Module .\scripts\pos-ui.psm1 -Force
Stop-PosUi -All -Force
Start-PosUi -Build -ClientWidth 1280 -ClientHeight 800
Enter-PosUiLogin
Add-PosUiProduct -Search 'Aceite'
Save-PosUiCapture -Path .\capturas\automatizacion\venta.png
Stop-PosUi
```

O todo junto, con base temporal y caja abierta:

```powershell
.\scripts\demo-venta-ui.ps1 -Build
.\scripts\demo-venta-ui.ps1 -Cobro
```

### 2.3 Funciones del módulo

| Función | Qué hace |
| --- | --- |
| `Start-PosUi` | Compila si hace falta, abre la ventana, espera y ajusta el cliente |
| `Stop-PosUi` | Cierra la ventana; `-All` cierra todos los `pos.exe` |
| `Get-PosUiRect` | Rectángulo de la ventana en pantalla |
| `Get-PosUiClientRect` | Tamaño del área de cliente |
| `Set-PosUiClientSize` | Fija el cliente exacto (1280×800, 1366×768…) |
| `ConvertTo-PosUiScreen` | Convierte un punto de la captura a pantalla |
| `Invoke-PosUiClick` | Clic izquierdo (o doble) en coordenadas de imagen |
| `Invoke-PosUiHover` | Mueve el ratón y espera (para tooltips) |
| `Invoke-PosUiWheel` | Rueda del ratón para bajar o subir una pantalla larga |
| `Send-PosUiKeys` | Envía teclas de `SendKeys` (`{F2}`, `{ENTER}`, `^a`…) |
| `Send-PosUiText` | Escribe texto escapando los caracteres especiales |
| `Save-PosUiCapture` | Captura con `PrintWindow` y guarda PNG |
| `Get-PosUiPixel` | Lee un píxel de la captura (para esperas) |
| `Wait-PosUiMain` | Espera a que la sidebar oscura esté visible |
| `Wait-PosUiLogin` | Espera a que aparezca el login |
| `Enter-PosUiLogin` | Pica la tarjeta de admin y teclea el PIN |
| `Open-PosUiScreen` | Navega con F2/F3/F5…F10 o por la sidebar |
| `Set-PosUiQuantity` | Escribe la cantidad en el campo “Cant.” |
| `Add-PosUiProduct` | Busca y agrega el primer resultado |
| `Add-PosUiCashProduct` | Cantidad + producto en una llamada |

### 2.4 Tamaños y coordenadas

Las coordenadas por omisión están calibradas para **1280×800 de cliente**. La
ventana se coloca donde el sistema quiera (en este equipo, `(234,234)`), pero
la fórmula usa el origen real, así que la posición no importa.

Para otro tamaño:

```powershell
Set-PosUiClientSize -Width 1366 -Height 768
Save-PosUiCapture -Path .\capturas\venta-1366.png
```

Los atajos de teclado funcionan en cualquier tamaño: `F2` Venta, `F3` Catálogo,
`F5` Conteo, `F6` Auditoría, `F7` Clientes, `F8` Reportes, `F9` Caja, `F10`
Configuración. Para pantallas sin atajo (Pedidos) hay que picar la sidebar; su
posición cambia entre sidebar ancha (212 px) y rail (64 px) a ≤1180 px.

### 2.5 Qué hace `Start-PosUi` y por qué

1. Se niega a arrancar si ya hay otro `pos.exe` (SQLite y la ventana se
   confunden; usa `Stop-PosUi -All`).
2. Compila con `-Build` o si no existe el ejecutable.
3. Espera a que aparezca `MainWindowHandle`.
4. **Espera ~2 s antes de redimensionar.** eframe todavía está creando el
   contexto OpenGL; un `WM_SIZE` a destiempo deja la ventana sin renderer y el
   log dice `egui_glow requires opengl 2.0+`.
5. Comprueba que la ventana tenga título. Si está vacío, OpenGL falló.
6. Ajusta el cliente al tamaño pedido con `SetWindowPos`, sumando el marco.

No quites la espera de 2 s “para ir más rápido”: es justo lo que evita capturas
negras y fallos intermitentes.

### 2.6 Capturas

```powershell
Save-PosUiCapture -Path .\capturas\automatizacion\venta-1366x768.png
```

- Usa `PrintWindow` con la bandera `2` (`PW_RENDERFULLCONTENT`).
- La imagen incluye el marco; es intencional para que las coordenadas coincidan.
- Nunca uses `CopyFromScreen`: en una sesión remota o con la ventana tapada
  sale otra cosa.
- La app tiene que estar en una sesión de escritorio real. `PrintWindow` no
  funciona en un servicio ni en una sesión sin escritorio.

### 2.7 Login

`Enter-PosUiLogin` asume 1280×800:

- tarjeta de admin en la imagen: `(476, 333)`
- teclado numérico: `1` en `(541,416)`, `2` en `(656,416)`… `0` en `(656,630)`
- confirmar en `(769,630)`

Si el PIN falla, el teclado baja ~14 px y las coordenadas siguientes ya no
sirven. Vuelve a capturar y recalcula; no encadenes clics a ciegas.

Para `ana`/`4321` hay que picar su tarjeta, que está en otra posición. Si vas a
automatizar varios usuarios, agrega el parámetro de coordenadas o usa el login
headless para preparar la sesión.

### 2.8 Flujo recomendado para cambiar la interfaz

1. `Stop-PosUi -All -Force` (el `.exe` está bloqueado mientras corre).
2. Edita `crates/pos-ui/src/app.rs` o `theme.rs`.
3. `cargo build -p pos-app`.
4. `.\scripts\demo-venta-ui.ps1` (base limpia, caja abierta, dos productos).
5. Captura a 1280×800, 1366×768 y 1024×600.
6. Revisa **texto cortado, solapamientos, filas a media altura, botones fuera
   de pantalla y contraste**. Si algo se movió, vuelve a capturar.
7. Prueba el flujo headless (`headless-smoke.ps1`) para asegurarte de que no
   rompiste la lógica.

### 2.9 Problemas conocidos

| Síntoma | Causa / solución |
| --- | --- |
| `No se puede especificar el parámetro 'Db'...` | PowerShell tiene un alias `Db`→`Debug`; usa `-Database` |
| La ventana abre negra o sin título | OpenGL falló; revisa `%LOCALAPPDATA%\PuntoDeVenta\error.log` |
| `egui_glow requires opengl 2.0+` | No uses `POS_SOFTWARE_GL=1` en este equipo: en esta versión también falla. Actualiza el driver o usa una sesión con GPU |
| `cargo build` dice “Acceso denegado” al escribir `pos.exe` | Hay un `pos.exe` corriendo; `Stop-PosUi -All -Force` |
| El clic cae desplazado | Se sumó `ClientToScreen`; usa la fórmula de `GetWindowRect` |
| La captura sale de otro tamaño | `Set-PosUiClientSize` ajusta el cliente, no la ventana completa |
| `PrintWindow` devuelve `false` | La ventana está minimizada o en otra sesión; restáurala con `ShowWindow` |
| El teclado del PIN se movió | El PIN anterior falló; vuelve a capturar y recalcula |

---

## 3. Archivos de esta automatización

| Archivo | Para qué |
| --- | --- |
| `crates/pos-app/src/guion.rs` | Ejecutor de guiones headless |
| `crates/pos-app/src/main.rs` | CLI `--guion`, `--headless`, `--db` |
| `scripts/pos-ui.psm1` | Módulo de ventana, clics, teclas y capturas |
| `scripts/demo-venta-ui.ps1` | Demo con base temporal y caja abierta |
| `scripts/headless-smoke.ps1` | Prueba HTTP completa sin ventana |
| `ejemplos/guion-venta.json` | Guion de venta de punta a punta |
| `ejemplos/guion-preparar-caja.json` | Abre caja para el demo de UI |
| `capturas/automatizacion/` | Capturas y reportes generados |

Si agregas una función al módulo, documéntala aquí y en `scripts/README.md`.
Si agregas un comando al API, el guion lo puede usar sin tocar el ejecutor:
sólo hay que escribir su JSON.
