# Construye el punto de venta en Windows (nativo, sin Linux ni mingw).
#
# Uso (PowerShell):
#   .\packaging\construir-windows.ps1           # pos.exe en target\release
#   .\packaging\construir-windows.ps1 instalador # + instalador NSIS en packaging\dist
#
# Requisitos: Rust (rustup.rs) y, sólo para el instalador, NSIS (makensis en PATH).
# Si el video es viejo y la ventana no abre:  $env:POS_SOFTWARE_GL=1; .\target\release\pos.exe
$ErrorActionPreference = "Stop"
$VERSION = "0.6.2"
$RAIZ = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
if (-not $RAIZ) { $RAIZ = (Get-Location).Path }
$DIST = Join-Path $RAIZ "packaging\dist"
$exe = Join-Path $RAIZ "target\release\pos.exe"

Write-Host "==> Compilando en modo release (Windows nativo)..."
& cargo build --release -p pos-app --bin pos
if ($LASTEXITCODE -ne 0) { exit 1 }

if (-not (Test-Path $exe)) {
  Write-Host "!! No encontré el binario en $exe" -ForegroundColor Red
  exit 1
}
$bytes = (Get-Item $exe).Length
Write-Host ("    binario: {0:N0} bytes -> {1}" -f $bytes, $exe)

New-Item -ItemType Directory -Force -Path $DIST | Out-Null

if ($args -contains "instalador" -or $args -contains "todo" -or $args.Count -eq 0) {
  $makensis = Get-Command makensis -ErrorAction SilentlyContinue
  if ($null -eq $makensis) {
    Write-Host "    aviso: no hay makensis en PATH. Instala NSIS para el instalador."
    Write-Host "    El pos.exe ya quedó en target\release\pos.exe y abre con doble clic."
  } else {
    Write-Host "==> Armando el instalador NSIS..."
    # La ruta real puede tener espacios y acentos ("Nueva carpeta (15)"):
    # se copia a pos-stage.exe junto al .nsi y se empaqueta relativo.
    $winDir = Join-Path $RAIZ "packaging\windows"
    Copy-Item -Force $exe (Join-Path $winDir "pos-stage.exe")
    try {
      Push-Location $winDir
      & makensis "-DVERSION=$VERSION" "-DPOS_EXE=pos-stage.exe" instalador.nsi
      if ($LASTEXITCODE -ne 0) { Pop-Location; exit 1 }
      $gen = Join-Path $winDir "PuntoDeVenta-$VERSION-Instalador.exe"
      if (Test-Path $gen) {
        Move-Item -Force $gen (Join-Path $DIST "PuntoDeVenta-$VERSION-Instalador.exe")
        Write-Host "    Instalador: packaging\dist\PuntoDeVenta-$VERSION-Instalador.exe"
      }
      Pop-Location
    } finally {
      Remove-Item -Force (Join-Path $winDir "pos-stage.exe") -ErrorAction SilentlyContinue
    }
  }
}

Write-Host "==> Verificando versión y generando SHA256..."
& $exe --version
if ($LASTEXITCODE -ne 0) { exit 1 }
$hash = (Get-FileHash -Algorithm SHA256 $exe).Hash.ToLower()
$shaFile = Join-Path $DIST "pos-$VERSION-win64-SHA256.txt"
"$hash  pos.exe" | Out-File -Encoding ascii $shaFile
Write-Host "    SHA256: $hash"
Write-Host "    guardado en: $shaFile"
$inst = Join-Path $DIST "PuntoDeVenta-$VERSION-Instalador.exe"
if (Test-Path $inst) {
  $hashInst = (Get-FileHash -Algorithm SHA256 $inst).Hash.ToLower()
  "$hashInst  PuntoDeVenta-$VERSION-Instalador.exe" | Out-File -Encoding ascii (Join-Path $DIST "PuntoDeVenta-$VERSION-Instalador-SHA256.txt")
  Write-Host "    SHA256 instalador: $hashInst"
}
Write-Host ""
Write-Host "==> Listo. Para abrir la caja:"
Write-Host "    .\target\release\pos.exe"
Write-Host "    .\target\release\pos.exe --checar          # prueba sin ventana"
Write-Host "    .\target\release\pos.exe --probar-login    # entra, busca y sale"
Write-Host ""
Write-Host "    Publica el .exe junto a su .txt de SHA256 para que la tienda"
Write-Host "    verifique antes de actualizar. Ver docs/firma-windows.md."
