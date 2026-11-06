# Demo de la interfaz real: base temporal + caja abierta + venta + captura.
#
# Es el ejemplo que conviene copiar para probar cualquier pantalla. Hace esto:
#
#   1. Crea una base temporal (no toca la base de la tienda).
#   2. Con `pos --guion` abre la caja con $500 de fondo, sin ventana.
#   3. Abre la interfaz contra esa misma base.
#   4. Entra como admin, agrega dos productos y captura la venta.
#   5. Cierra la ventana y borra la base temporal.
#
# Uso:
#   .\scripts\demo-venta-ui.ps1
#   .\scripts\demo-venta-ui.ps1 -Build
#   .\scripts\demo-venta-ui.ps1 -Cobro
#   .\scripts\demo-venta-ui.ps1 -ClientWidth 1366 -ClientHeight 768 -KeepDb
#
# Las coordenadas del módulo están calibradas a 1280x800. Si cambias el tamaño,
# usa los atajos de teclado (F2, F9...) y revisa la captura antes de confiar en
# un clic. Para 1366x768 y 1024x600 hay ejemplos en capturas\automatizacion\.

[CmdletBinding()]
param(
    [int]$ClientWidth = 1280,
    [int]$ClientHeight = 800,
    [string]$OutputDir = "",
    [string]$Database = "",
    [switch]$Build,
    [switch]$Cobro,
    [switch]$KeepDb
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$raiz = Split-Path -Parent $PSScriptRoot
Import-Module (Join-Path $PSScriptRoot 'pos-ui.psm1') -Force

$exe = Join-Path $raiz 'target\debug\pos.exe'
if ($Build -or -not (Test-Path $exe)) {
    Push-Location $raiz
    try {
        & cargo build -p pos-app
        if ($LASTEXITCODE -ne 0) { throw "cargo build -p pos-app falló." }
    } finally {
        Pop-Location
    }
}

$temporal = $false
if (-not $Database) {
    $Database = Join-Path $env:TEMP ("pos-demo-ui-" + [guid]::NewGuid().ToString('N') + ".db")
    $temporal = $true
}
if (-not $OutputDir) {
    $OutputDir = Join-Path $raiz 'capturas\automatizacion'
}

$capturaVenta = Join-Path $OutputDir 'demo-venta.png'
$capturaCobro = Join-Path $OutputDir 'demo-cobro.png'

try {
    Write-Host "Base temporal: $Database"

    # La caja se abre por la API, sin ventana: el demo de UI empieza con el
    # mismo estado que tendría una tienda a media mañana.
    $preparar = Join-Path $raiz 'ejemplos\guion-preparar-caja.json'
    & $exe --db $Database --guion $preparar
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo preparar la caja (código $LASTEXITCODE)."
    }

    Start-PosUi -Exe $exe -Database $Database `
        -ClientWidth $ClientWidth -ClientHeight $ClientHeight | Out-Null

    Enter-PosUiLogin | Out-Null
    Add-PosUiProduct -Search 'Aceite'
    Add-PosUiProduct -Search 'Coca'

    $ruta = Save-PosUiCapture -Path $capturaVenta
    Write-Host "Captura de venta: $ruta"

    if ($Cobro) {
        # Botón COBRAR de la barra del ticket (1280x800).
        Invoke-PosUiClick -X 1157 -Y 683 -DelayMs 900
        $ruta = Save-PosUiCapture -Path $capturaCobro
        Write-Host "Captura de cobro: $ruta"
    }
}
finally {
    Stop-PosUi -All -Force
    if ($temporal -and -not $KeepDb) {
        Remove-Item $Database, "$Database-wal", "$Database-shm" -Force -ErrorAction SilentlyContinue
    } elseif ($KeepDb) {
        Write-Host "Base conservada: $Database"
    }
}
