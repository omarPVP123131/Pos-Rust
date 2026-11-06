# Scripts

Atajos para arrancar, probar y capturar el punto de venta. La guía completa de
automatización está en [`../docs/automatizacion.md`](../docs/automatizacion.md).

## Para abrir la tienda

| Script | Qué hace |
| --- | --- |
| `iniciar.bat` | Doble clic: compila en release si hace falta y abre la caja |
| `iniciar.ps1` | Lo mismo desde PowerShell |

```powershell
.\scripts\iniciar.ps1
```

## Para probar sin ventana

### `headless-smoke.ps1`

Levanta `pos --headless` contra una base temporal y comprueba el día completo
por HTTP: salud, login, caja cerrada, abrir caja, buscar, cobrar, recientes,
caja con el ticket y cierre.

```powershell
.\scripts\headless-smoke.ps1
.\scripts\headless-smoke.ps1 -Build
.\scripts\headless-smoke.ps1 -Port 18080 -KeepDb
```

| Parámetro | Por omisión | Qué hace |
| --- | --- | --- |
| `-Exe` | `target\debug\pos.exe` | Ejecutable a probar |
| `-Port` | uno libre | Puerto HTTP |
| `-Database` | base temporal | Base de datos (no uses la real) |
| `-ReportPath` | `capturas\automatizacion\headless-smoke-reporte.json` | Reporte JSON |
| `-Build` | no | Compila antes de probar |
| `-KeepDb` | no | No borra la base al terminar |

Sale con código `0` si las 9 comprobaciones pasan.

### Guiones `--guion`

No necesitan PowerShell: son JSON y los ejecuta el propio `pos.exe`.

```powershell
.\target\debug\pos.exe --db "$env:TEMP\pos-guion.db" --guion .\ejemplos\guion-venta.json
```

- `ejemplos/guion-venta.json`: venta completa, caja y reporte.
- `ejemplos/guion-preparar-caja.json`: deja una caja abierta con $500.

Formato y catálogo de comandos: [`../docs/automatizacion.md`](../docs/automatizacion.md).

## Para probar la interfaz real

### `demo-venta-ui.ps1`

El ejemplo más útil: base temporal, caja abierta por API, login, dos productos
y captura. No toca la base de la tienda.

```powershell
.\scripts\demo-venta-ui.ps1
.\scripts\demo-venta-ui.ps1 -Build
.\scripts\demo-venta-ui.ps1 -Cobro
.\scripts\demo-venta-ui.ps1 -ClientWidth 1366 -ClientHeight 768 -KeepDb
```

| Parámetro | Por omisión | Qué hace |
| --- | --- | --- |
| `-ClientWidth` / `-ClientHeight` | `1280` / `800` | Tamaño del área de cliente |
| `-OutputDir` | `capturas\automatizacion` | Carpeta de las capturas |
| `-Database` | base temporal | Base a usar |
| `-Build` | no | Compila antes |
| `-Cobro` | no | Además abre el diálogo de cobro y lo captura |
| `-KeepDb` | no | Conserva la base temporal |

> Las coordenadas del módulo están calibradas a **1280×800**. Si cambias el
> tamaño, navega con los atajos y revisa la captura antes de encadenar clics.

### `pos-ui.psm1`

Módulo para escribir tus propias pruebas de ventana. Lo importante:

```powershell
Import-Module .\scripts\pos-ui.psm1 -Force
Stop-PosUi -All -Force
Start-PosUi -Build -ClientWidth 1366 -ClientHeight 768
Enter-PosUiLogin
Add-PosUiProduct -Search 'Aceite'
Save-PosUiCapture -Path .\capturas\automatizacion\venta-1366x768.png
Stop-PosUi
```

Funciones: `Start-PosUi`, `Stop-PosUi`, `Get-PosUiRect`,
`Get-PosUiClientRect`, `Set-PosUiClientSize`, `ConvertTo-PosUiScreen`,
`Invoke-PosUiClick`, `Invoke-PosUiHover`, `Invoke-PosUiWheel`, `Send-PosUiKeys`, `Send-PosUiText`,
`Save-PosUiCapture`, `Get-PosUiPixel`, `Wait-PosUiMain`, `Wait-PosUiLogin`,
`Enter-PosUiLogin`, `Open-PosUiScreen`, `Set-PosUiQuantity`,
`Add-PosUiProduct`, `Add-PosUiCashProduct`.

Regla de coordenadas: `pantalla = (rect.Left + x_imagen, rect.Top + y_imagen)`.
La captura de `PrintWindow` incluye el marco, igual que `GetWindowRect`. No se
suma `ClientToScreen`.

Notas:

- Usa `-Database`, no `-Db`: PowerShell tiene un alias `Db` → `Debug`.
- `Start-PosUi` espera ~2 s antes de redimensionar para no romper el contexto
  OpenGL. No quites esa espera.
- Sólo puede haber un `pos.exe`. Si hay otro, `Stop-PosUi -All -Force`.
- `PrintWindow` necesita una sesión de escritorio real.

## Utilidades

| Script | Qué hace |
| --- | --- |
| `cliente_movil.py` | Cliente de la LAN sin dependencias; usa la misma API que la app móvil |
| `armar_galeria.py` | Regenera `docs/galeria.html` con las capturas de `crates/pos-ui/tests/snapshots/` |

```powershell
python .\scripts\cliente_movil.py http://127.0.0.1:8080 --completo
python .\scripts\armar_galeria.py
```
