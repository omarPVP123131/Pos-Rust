# Prueba de humo del modo headless (sin ventana).
#
# Levanta `pos.exe --headless` contra una base temporal, habla con la misma API
# HTTP que usa la app móvil y comprueba el flujo completo:
#
#   login -> abrir caja -> buscar producto -> cobrar -> ver venta -> cerrar caja
#
# No toca la base real. Al terminar apaga el servidor y borra la base temporal
# (salvo que se pase -KeepDb).
#
# Uso:
#   .\scripts\headless-smoke.ps1
#   .\scripts\headless-smoke.ps1 -Build
#   .\scripts\headless-smoke.ps1 -Port 18080 -KeepDb

[CmdletBinding()]
param(
    [string]$Exe,
    [int]$Port = 0,
    [string]$Database,
    [string]$ReportPath = "",
    [switch]$Build,
    [switch]$KeepDb
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$raiz = Split-Path -Parent $PSScriptRoot
if (-not $Exe) {
    $Exe = Join-Path $raiz 'target\debug\pos.exe'
    if ($Build -or -not (Test-Path $Exe)) {
        Push-Location $raiz
        try {
            & cargo build -p pos-app
            if ($LASTEXITCODE -ne 0) { throw "cargo build -p pos-app falló." }
        } finally {
            Pop-Location
        }
    }
}
if (-not (Test-Path $Exe)) { throw "No existe el ejecutable: $Exe" }

if (-not $Database) {
    $Database = Join-Path $env:TEMP ("pos-headless-" + [guid]::NewGuid().ToString('N') + ".db")
}
if ($Port -eq 0) {
    $escucha = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
    $escucha.Start()
    $Port = $escucha.LocalEndpoint.Port
    $escucha.Stop()
}
if (-not $ReportPath) {
    $ReportPath = Join-Path $raiz 'capturas\automatizacion\headless-smoke-reporte.json'
}

$log = Join-Path $env:TEMP "pos-headless-$Port.log"
$err = Join-Path $env:TEMP "pos-headless-$Port.err.log"
$script:Token = $null
$script:Port = $Port
$resultados = New-Object System.Collections.Generic.List[object]

function Add-Resultado {
    param(
        [string]$Nombre,
        [bool]$Ok,
        [string]$Detalle
    )
    $script:resultados.Add([pscustomobject]@{
        paso   = $Nombre
        ok     = $Ok
        detalle = $Detalle
    })
    $marca = if ($Ok) { 'OK' } else { 'FALLO' }
    Write-Host ("[{0}] {1} ... {2}" -f $script:resultados.Count, $Nombre, $marca)
    if ($Detalle) { Write-Host ("       {0}" -f $Detalle) }
}

function Invoke-PosApi {
    param(
        [Parameter(Mandatory = $true)]$Comando,
        [switch]$AllowError
    )

    $cuerpo = @{
        token   = $script:Token
        comando = $Comando
    } | ConvertTo-Json -Depth 40 -Compress

    $respuesta = Invoke-RestMethod -Uri "http://127.0.0.1:$($script:Port)/api" `
        -Method Post -ContentType 'application/json; charset=utf-8' -Body $cuerpo

    if ($respuesta.PSObject.Properties.Name -contains 'Error') {
        if ($AllowError) { return $respuesta }
        throw "La API respondió error: $($respuesta.Error)"
    }
    return $respuesta.Ok
}

function Wait-PosHealth {
    param([int]$TimeoutSeconds = 20)

    $limite = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $limite) {
        try {
            $salud = Invoke-RestMethod -Uri "http://127.0.0.1:$($script:Port)/api/salud" -TimeoutSec 2
            if ($salud.estado -eq 'ok') { return $true }
        } catch {
            # Todavía no escucha: se reintenta.
        }
        Start-Sleep -Milliseconds 250
    }
    throw "El servidor headless no respondió en $TimeoutSeconds s. Revisa $log"
}

$proceso = $null
try {
    Write-Host "Base temporal: $Database"
    Write-Host "Puerto: $Port"
    $proceso = Start-Process -FilePath $Exe `
        -ArgumentList @('--headless', '--puerto', "$Port", '--db', $Database) `
        -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $log -RedirectStandardError $err

    Wait-PosHealth -TimeoutSeconds 20 | Out-Null
    Add-Resultado 'El servidor headless responde /api/salud' $true "http://127.0.0.1:$Port"

    # 1. Login
    $sesion = Invoke-PosApi @{ Login = @{ username = 'admin'; pin = '1234' } }
    $script:Token = $sesion.Sesion.token
    Add-Resultado 'Login de admin' ([bool]$script:Token) "usuario: $($sesion.Sesion.usuario.display_name)"

    # 2. Caja cerrada al arrancar
    $caja = Invoke-PosApi 'EstadoCaja'
    Add-Resultado 'La caja empieza cerrada' (-not $caja.Caja.abierta) "abierta=$($caja.Caja.abierta)"

    # 3. Abrir caja
    $caja = Invoke-PosApi @{ AbrirCaja = @{ fondo_cents = 10000 } }
    $caja = Invoke-PosApi 'EstadoCaja'
    Add-Resultado 'Abrir caja con $100.00 de fondo' ($caja.Caja.abierta -and $caja.Caja.fondo_cents -eq 10000) `
        "fondo=$($caja.Caja.fondo_cents) centavos"

    # 4. Buscar producto
    $busqueda = Invoke-PosApi @{ BuscarProductos = @{ termino = 'Aceite'; limite = 5 } }
    $producto = $busqueda.Productos | Select-Object -First 1
    if (-not $producto) { throw "La búsqueda de 'Aceite' no devolvió productos." }
    Add-Resultado 'Buscar producto' ([bool]$producto) "$($producto.sku) · $($producto.name) · $($producto.price_cents) centavos"

    # 5. Cobrar una pieza en efectivo
    $linea = @{
        sku             = $producto.sku
        descripcion     = $producto.name
        qty_millis      = 1000
        precio_cents    = $producto.price_cents
        descuento_cents = 0
    }
    $pago = @{
        metodo         = 'efectivo'
        monto_cents    = $producto.price_cents
        recibido_cents = $producto.price_cents + 1000
    }
    $venta = @{
        lineas                 = @($linea)
        descuento_global_cents = 0
        pagos                  = @($pago)
        cliente_id             = $null
        nota                   = 'Prueba headless'
        supervisor             = $null
    }
    $guardada = Invoke-PosApi @{ Cobrar = @{ venta = $venta } }
    $okVenta = $guardada.Venta.total_cents -eq $producto.price_cents -and $guardada.Venta.cambio_cents -eq 1000
    Add-Resultado 'Cobrar una pieza' $okVenta `
        "folio=$($guardada.Venta.folio) total=$($guardada.Venta.total_cents) cambio=$($guardada.Venta.cambio_cents)"

    # 6. La venta aparece en recientes
    $recientes = Invoke-PosApi @{ VentasRecientes = @{ limite = 5 } }
    $encontrada = @($recientes.Ventas | Where-Object { $_.folio -eq $guardada.Venta.folio }).Count -gt 0
    Add-Resultado 'La venta aparece en recientes' $encontrada "folio=$($guardada.Venta.folio)"

    # 7. La caja refleja la venta
    $caja = Invoke-PosApi 'EstadoCaja'
    $okCaja = $caja.Caja.tickets -eq 1 -and $caja.Caja.vendido_cents -eq $producto.price_cents
    Add-Resultado 'La caja suma el ticket' $okCaja `
        "tickets=$($caja.Caja.tickets) vendido=$($caja.Caja.vendido_cents)"

    # 8. Cerrar caja con autorización de admin
    $cierre = Invoke-PosApi @{
        CerrarCaja = @{
            piezas     = @()
            supervisor = @{ username = 'admin'; pin = '1234' }
        }
    }
    $caja = Invoke-PosApi 'EstadoCaja'
    Add-Resultado 'Cerrar caja' (-not $caja.Caja.abierta) "abierta=$($caja.Caja.abierta)"
}
catch {
    Add-Resultado 'Excepción durante el flujo' $false $_.Exception.Message
}
finally {
    if ($proceso -and -not $proceso.HasExited) {
        Stop-Process -Id $proceso.Id -Force
    }
    if (-not $KeepDb) {
        Remove-Item $Database, "$Database-wal", "$Database-shm" -Force -ErrorAction SilentlyContinue
    }

    $fallos = @($resultados | Where-Object { -not $_.ok }).Count
    $reporte = [pscustomobject]@{
        fecha      = (Get-Date).ToString('s')
        exe        = $Exe
        base       = $Database
        puerto     = $Port
        total      = $resultados.Count
        fallos     = $fallos
        resultados = $resultados
    }
    $dir = Split-Path -Parent $ReportPath
    if ($dir -and -not (Test-Path $dir)) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }
    $reporte | ConvertTo-Json -Depth 20 | Set-Content -Path $ReportPath -Encoding UTF8

    Write-Host ""
    Write-Host ("Resumen: {0}/{1} OK, {2} fallo(s)" -f ($resultados.Count - $fallos), $resultados.Count, $fallos)
    Write-Host "Reporte: $ReportPath"
    if ($fallos -gt 0) {
        Write-Host "Log del servidor: $log"
        exit 1
    }
}
