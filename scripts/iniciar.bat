@echo off
REM Doble clic para abrir la caja en Windows. Compila la primera vez.
setlocal
cd /d "%~dp0.."
where cargo >nul 2>nul
if errorlevel 1 (
  echo Instala Rust primero: https://rustup.rs
  echo Despues vuelve a dar doble clic a este archivo.
  pause
  exit /b 1
)
if not exist "target\release\pos.exe" (
  echo Compilando por primera vez (tarda unos minutos)...
  cargo build --release -p pos-app --bin pos
  if errorlevel 1 (
    echo No se pudo compilar. Revisa el mensaje de arriba.
    pause
    exit /b 1
  )
)
echo Abriendo el punto de venta...
"target\release\pos.exe"
if errorlevel 1 (
  echo.
  echo Si dice OpenGL/WGL/context: actualiza el driver de video o prueba con software:
  echo   set POS_SOFTWARE_GL=1 ^&^& "target\release\pos.exe"
  echo Revisa %LOCALAPPDATA%\PuntoDeVenta\error.log
  pause
)
