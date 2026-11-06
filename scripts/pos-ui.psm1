# Módulo de automatización de la interfaz de Punto de Venta.
#
# La regla que ahorra horas: la captura con PrintWindow mide exactamente lo
# mismo que GetWindowRect. Por eso, para convertir un punto de la captura a la
# pantalla se suma el origen de la ventana, NO el origen del cliente:
#
#     pantalla = (rect.Left + x_imagen, rect.Top + y_imagen)
#
# Si se usa ClientToScreen se suman dos veces el borde y la barra de título y
# los clics caen ~8 px a la izquierda y ~28 px arriba. Eso fue justo lo que
# hizo perder tiempo antes.
#
# Uso rápido:
#   Import-Module .\scripts\pos-ui.psm1 -Force
#   Start-PosUi -Build
#   Enter-PosUiLogin
#   Add-PosUiProduct -Search 'Aceite'
#   Save-PosUiCapture -Path .\capturas\venta.png

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not ('PosUiWin' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;

public class PosUiWin {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, IntPtr extra);
    [DllImport("user32.dll", EntryPoint = "mouse_event")] public static extern void mouse_event_wheel(uint flags, uint dx, uint dy, int data, IntPtr extra);

    public struct RECT { public int Left, Top, Right, Bottom; }
}
'@
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$script:PosUiProcess = $null

function Get-PosUiProcess {
    [CmdletBinding()]
    param([switch]$AllowMissing)

    if ($script:PosUiProcess -and -not $script:PosUiProcess.HasExited) {
        return $script:PosUiProcess
    }
    $p = Get-Process -Name pos -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $p) {
        if ($AllowMissing) { return $null }
        throw "No hay ningún pos.exe abierto. Usa Start-PosUi."
    }
    $script:PosUiProcess = $p
    return $p
}

function Get-PosUiRect {
    [CmdletBinding()]
    param()

    $p = Get-PosUiProcess
    $r = New-Object PosUiWin+RECT
    [PosUiWin]::GetWindowRect($p.MainWindowHandle, [ref]$r) | Out-Null
    [pscustomobject]@{
        Left   = $r.Left
        Top    = $r.Top
        Right  = $r.Right
        Bottom = $r.Bottom
        Width  = $r.Right - $r.Left
        Height = $r.Bottom - $r.Top
    }
}

function Get-PosUiClientRect {
    [CmdletBinding()]
    param()

    $p = Get-PosUiProcess
    $r = New-Object PosUiWin+RECT
    [PosUiWin]::GetClientRect($p.MainWindowHandle, [ref]$r) | Out-Null
    [pscustomobject]@{
        Width  = $r.Right - $r.Left
        Height = $r.Bottom - $r.Top
    }
}

function Set-PosUiClientSize {
    [CmdletBinding()]
    param(
        [int]$Width = 1280,
        [int]$Height = 800
    )

    $p = Get-PosUiProcess
    $h = $p.MainWindowHandle
    [PosUiWin]::ShowWindow($h, 9) | Out-Null
    Start-Sleep -Milliseconds 150

    $wr = New-Object PosUiWin+RECT
    [PosUiWin]::GetWindowRect($h, [ref]$wr) | Out-Null
    $cr = New-Object PosUiWin+RECT
    [PosUiWin]::GetClientRect($h, [ref]$cr) | Out-Null

    $marcoAncho = ($wr.Right - $wr.Left) - ($cr.Right - $cr.Left)
    $marcoAlto = ($wr.Bottom - $wr.Top) - ($cr.Bottom - $cr.Top)

    # SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
    [PosUiWin]::SetWindowPos(
        $h, [IntPtr]::Zero, 0, 0,
        ($Width + $marcoAncho), ($Height + $marcoAlto),
        0x0016
    ) | Out-Null
    Start-Sleep -Milliseconds 500
    return (Get-PosUiRect)
}

function ConvertTo-PosUiScreen {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][int]$X,
        [Parameter(Mandatory = $true)][int]$Y
    )

    $r = Get-PosUiRect
    [pscustomobject]@{
        X = $r.Left + $X
        Y = $r.Top + $Y
    }
}

function Start-PosUi {
    [CmdletBinding()]
    param(
        [string]$Exe,
        [string]$Database,
        [switch]$Build,
        [switch]$DryRun,
        [int]$ClientWidth = 1280,
        [int]$ClientHeight = 800,
        [int]$TimeoutSeconds = 20
    )

    $abiertos = @(Get-Process -Name pos -ErrorAction SilentlyContinue)
    if ($abiertos.Count -gt 0) {
        throw "Ya hay $($abiertos.Count) pos.exe abierto(s). Usa Stop-PosUi -All antes de Start-PosUi."
    }

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

    $argumentos = @()
    if ($Database) { $argumentos += @('--db', $Database) }
    if ($DryRun) { $argumentos += '--dry-run' }

    $p = Start-Process -FilePath $Exe -ArgumentList $argumentos -PassThru
    $script:PosUiProcess = $p

    $limite = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $limite) {
        $p.Refresh()
        if ($p.HasExited) { throw "pos.exe terminó al arrancar (código $($p.ExitCode))." }
        if ($p.MainWindowHandle -ne 0) { break }
        Start-Sleep -Milliseconds 200
    }
    if ($p.MainWindowHandle -eq 0) { throw "pos.exe no abrió ventana en $TimeoutSeconds s." }

    # No redimensionar en el primer instante: eframe todavía está creando el
    # contexto OpenGL y un WM_SIZE a destiempo puede dejarlo sin renderer
    # (la ventana queda negra y el log dice "requires opengl 2.0+").
    [PosUiWin]::ShowWindow($p.MainWindowHandle, 9) | Out-Null
    [PosUiWin]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 2000
    $p.Refresh()
    if ($p.HasExited) { throw "pos.exe cerró la ventana al iniciar. Revisa error.log." }
    if ([string]::IsNullOrWhiteSpace($p.MainWindowTitle)) {
        throw "La ventana abrió sin título; probablemente falló OpenGL. Revisa error.log."
    }

    $cliente = Get-PosUiClientRect
    if ($cliente.Width -ne $ClientWidth -or $cliente.Height -ne $ClientHeight) {
        Set-PosUiClientSize -Width $ClientWidth -Height $ClientHeight | Out-Null
        Start-Sleep -Milliseconds 600
    }
    [PosUiWin]::SetForegroundWindow($p.MainWindowHandle) | Out-Null

    [pscustomobject]@{
        Process = $p
        Handle  = $p.MainWindowHandle
        Rect    = (Get-PosUiRect)
    }
}

function Stop-PosUi {
    [CmdletBinding()]
    param(
        [switch]$Force,
        [switch]$All
    )

    $procesos = @()
    if ($All) {
        $procesos = @(Get-Process -Name pos -ErrorAction SilentlyContinue)
    } else {
        $p = Get-PosUiProcess -AllowMissing
        if ($p) { $procesos = @($p) }
    }
    foreach ($proceso in $procesos) {
        if (-not $proceso.HasExited) {
            $proceso.CloseMainWindow() | Out-Null
            if (-not $proceso.WaitForExit(3000)) {
                Stop-Process -Id $proceso.Id -Force
            }
        }
    }
    $script:PosUiProcess = $null
}

function Invoke-PosUiClick {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][int]$X,
        [Parameter(Mandatory = $true)][int]$Y,
        [switch]$DoubleClick,
        [int]$DelayMs = 350
    )

    $p = Get-PosUiProcess
    [PosUiWin]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
    $pt = ConvertTo-PosUiScreen -X $X -Y $Y
    [PosUiWin]::SetCursorPos($pt.X, $pt.Y) | Out-Null
    Start-Sleep -Milliseconds 120

    [PosUiWin]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero) # left down
    Start-Sleep -Milliseconds 90
    [PosUiWin]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero) # left up

    if ($DoubleClick) {
        Start-Sleep -Milliseconds 80
        [PosUiWin]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 90
        [PosUiWin]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
    }
    Start-Sleep -Milliseconds $DelayMs
}

function Invoke-PosUiHover {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][int]$X,
        [Parameter(Mandatory = $true)][int]$Y,
        [int]$DelayMs = 400
    )

    $p = Get-PosUiProcess
    [PosUiWin]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
    $pt = ConvertTo-PosUiScreen -X $X -Y $Y
    [PosUiWin]::SetCursorPos($pt.X, $pt.Y) | Out-Null
    Start-Sleep -Milliseconds $DelayMs
}

function Invoke-PosUiWheel {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][int]$X,
        [Parameter(Mandatory = $true)][int]$Y,
        [int]$Delta = -600,
        [int]$DelayMs = 500
    )

    $p = Get-PosUiProcess
    [PosUiWin]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
    $pt = ConvertTo-PosUiScreen -X $X -Y $Y
    [PosUiWin]::SetCursorPos($pt.X, $pt.Y) | Out-Null
    Start-Sleep -Milliseconds 120
    # 0x0800 = MOUSEEVENTF_WHEEL; delta positivo sube, negativo baja.
    [PosUiWin]::mouse_event_wheel(0x0800, 0, 0, $Delta, [IntPtr]::Zero)
    Start-Sleep -Milliseconds $DelayMs
}

function Send-PosUiKeys {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Keys,
        [int]$DelayMs = 300
    )

    $p = Get-PosUiProcess
    [PosUiWin]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 150
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds $DelayMs
}

function Send-PosUiText {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [int]$DelayMs = 300
    )

    # SendKeys trata estos caracteres como comandos: se escapan con llaves.
    $escapado = $Text -replace '([+^%~(){}\[\]])', '{$1}'
    Send-PosUiKeys -Keys $escapado -DelayMs $DelayMs
}

function Save-PosUiCapture {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Path
    )

    $p = Get-PosUiProcess
    $h = $p.MainWindowHandle
    [PosUiWin]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 250

    $r = Get-PosUiRect
    $bmp = New-Object System.Drawing.Bitmap($r.Width, $r.Height)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    $ok = [PosUiWin]::PrintWindow($h, $hdc, 2)
    $g.ReleaseHdc($hdc)
    $g.Dispose()

    $Path = [System.IO.Path]::GetFullPath($Path)
    $dir = Split-Path -Parent $Path
    if ($dir -and -not (Test-Path $dir)) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()

    if (-not $ok) {
        Write-Warning "PrintWindow devolvió false; la captura puede salir negra."
    }
    return $Path
}

function Get-PosUiPixel {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][int]$X,
        [Parameter(Mandatory = $true)][int]$Y
    )

    $p = Get-PosUiProcess
    $r = Get-PosUiRect
    $bmp = New-Object System.Drawing.Bitmap($r.Width, $r.Height)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [PosUiWin]::PrintWindow($p.MainWindowHandle, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc)
    $g.Dispose()

    $c = $bmp.GetPixel($X, $Y)
    $bmp.Dispose()
    [pscustomobject]@{
        R          = $c.R
        G          = $c.G
        B          = $c.B
        Brightness = [math]::Round(($c.R + $c.G + $c.B) / 3.0, 1)
    }
}

function Wait-PosUiMain {
    [CmdletBinding()]
    param([int]$TimeoutSeconds = 12)

    $limite = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $limite) {
        # En la pantalla principal el borde izquierdo es la sidebar oscura.
        # En el login esa zona es el fondo claro con degradado.
        $px = Get-PosUiPixel -X 30 -Y 100
        if ($px.Brightness -lt 120) { return $true }
        Start-Sleep -Milliseconds 300
    }
    throw "No se detectó la pantalla principal en $TimeoutSeconds s."
}

function Wait-PosUiLogin {
    [CmdletBinding()]
    param([int]$TimeoutSeconds = 12)

    $limite = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $limite) {
        $px = Get-PosUiPixel -X 30 -Y 100
        if ($px.Brightness -ge 120) { return $true }
        Start-Sleep -Milliseconds 300
    }
    throw "No se detectó la pantalla de login en $TimeoutSeconds s."
}

function Enter-PosUiLogin {
    [CmdletBinding()]
    param(
        [string]$Pin = '1234',
        [int]$UserCardX = 476,
        [int]$UserCardY = 333
    )

    $p = Get-PosUiProcess
    [PosUiWin]::SetForegroundWindow($p.MainWindowHandle) | Out-Null

    # Si ya estamos dentro, no hacer nada.
    $px = Get-PosUiPixel -X 30 -Y 100
    if ($px.Brightness -lt 120) { return $true }

    # Tarjeta del usuario (1280x800). En la base demo sólo existe admin.
    Invoke-PosUiClick -X $UserCardX -Y $UserCardY -DelayMs 700

    $teclas = @{
        '1' = @(541, 416); '2' = @(656, 416); '3' = @(769, 416)
        '4' = @(541, 487); '5' = @(656, 487); '6' = @(769, 487)
        '7' = @(541, 558); '8' = @(656, 558); '9' = @(769, 558)
        '0' = @(656, 630)
    }
    foreach ($digito in $Pin.ToCharArray()) {
        $pt = $teclas["$digito"]
        if (-not $pt) { throw "Dígito no soportado en el PIN: $digito" }
        Invoke-PosUiClick -X $pt[0] -Y $pt[1] -DelayMs 220
    }
    Invoke-PosUiClick -X 769 -Y 630 -DelayMs 900
    Wait-PosUiMain -TimeoutSeconds 12 | Out-Null
    return $true
}

function Open-PosUiScreen {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('Venta', 'Catalogo', 'Caja', 'Clientes', 'Pedidos', 'Reportes', 'Conteo', 'Auditoria', 'Configuracion')]
        [string]$Name
    )

    $atajos = @{
        Venta         = '{F2}'
        Catalogo      = '{F3}'
        Caja          = '{F9}'
        Clientes      = '{F7}'
        Reportes      = '{F8}'
        Conteo        = '{F5}'
        Auditoria     = '{F6}'
        Configuracion = '{F10}'
    }
    if ($atajos.ContainsKey($Name)) {
        Send-PosUiKeys -Keys $atajos[$Name] -DelayMs 700
        return
    }

    # Pedidos no tiene atajo: se pica en la sidebar. A 1280 px de cliente la
    # sidebar mide 212 px; en modo rail (<=1180) mide 64 px.
    $anchoCliente = (Get-PosUiClientRect).Width
    $x = if ($anchoCliente -le 1180) { 32 } else { 100 }
    $y = @{
        Venta = 156; Catalogo = 198; Caja = 240; Clientes = 282; Pedidos = 324
        Reportes = 394; Conteo = 437; Auditoria = 479; Configuracion = 551
    }[$Name]
    Invoke-PosUiClick -X $x -Y $y -DelayMs 700
}

function Set-PosUiQuantity {
    [CmdletBinding()]
    param([int]$Quantity = 1)

    Invoke-PosUiClick -X 692 -Y 128 -DelayMs 200
    Send-PosUiKeys -Keys '^a' -DelayMs 100
    Send-PosUiText -Text "$Quantity" -DelayMs 120
    Send-PosUiKeys -Keys '{ENTER}' -DelayMs 300
}

function Add-PosUiProduct {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Search,
        [int]$DelayMs = 600
    )

    Send-PosUiKeys -Keys '{F2}' -DelayMs 350
    Invoke-PosUiClick -X 400 -Y 128 -DelayMs 200
    Send-PosUiKeys -Keys '^a' -DelayMs 100
    Send-PosUiText -Text $Search -DelayMs 150
    Start-Sleep -Milliseconds 500
    Send-PosUiKeys -Keys '{ENTER}' -DelayMs $DelayMs
}

function Add-PosUiCashProduct {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Search,
        [int]$Quantity = 1
    )

    Set-PosUiQuantity -Quantity $Quantity
    Add-PosUiProduct -Search $Search
}

Export-ModuleMember -Function *
