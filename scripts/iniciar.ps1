# Abre la caja en Windows con un doble clic (o desde PowerShell).
# Compila si hace falta y luego abre la ventana.
$ErrorActionPreference = "Stop"
$RAIZ = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
if (-not $RAIZ -or -not (Test-Path (Join-Path $RAIZ "Cargo.toml"))) {
  # Si se copió suelto, usa la carpeta actual.
  $RAIZ = (Get-Location).Path
}
Push-Location $RAIZ
try {
  $exe = Join-Path $RAIZ "target\release\pos.exe"
  if (-not (Test-Path $exe)) {
    Write-Host "Compilando por primera vez (tarda unos minutos)..."
    & cargo build --release -p pos-app --bin pos
    if ($LASTEXITCODE -ne 0) { exit 1 }
  }
  Write-Host "Abriendo el punto de venta..."
  & $exe
} finally {
  Pop-Location
}
