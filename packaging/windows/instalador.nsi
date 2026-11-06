; ============================================================================
;  Instalador de Windows para el Punto de Venta (NSIS)
;
;  Se puede compilar en Windows (makensis instalador.nsi) y también en Linux
;  con el NSIS de Debian (apt install nsis), que es como se arma aquí:
;      cargo build --release --target x86_64-pc-windows-gnu
;      makensis -DVERSION=0.6.2 packaging/windows/instalador.nsi
;
;  Dos decisiones que se explican solas al instalarlo:
;
;  1. Se instala en %LOCALAPPDATA%\Programs, NO en "Archivos de programa".
;     La razón es el actualizador: para reemplazar pos.exe hay que poder
;     escribir en su carpeta, y en "Archivos de programa" eso exige permisos de
;     administrador. Instalado por usuario, el programa se actualiza sin que
;     nadie tenga que ser administrador ni aceptar una ventana de UAC.
;
;  2. Se abre el puerto en el firewall. Si no, Windows bloquea en silencio al
;     celular que intenta hablar con la caja por la red.
; ============================================================================

!include "MUI2.nsh"

!ifndef VERSION
    !define VERSION "0.6.2"
!endif

; Ruta del pos.exe a empaquetar. Cruzado desde Linux es
; target/x86_64-pc-windows-gnu/release/pos.exe; nativo en Windows es
; target/release/pos.exe. Se elige con -DPOS_EXE=... o se adivina.
!ifndef POS_EXE
    !define POS_EXE "..\..\target\x86_64-pc-windows-gnu\release\pos.exe"
!endif

Name "Punto de Venta"
OutFile "PuntoDeVenta-${VERSION}-Instalador.exe"
InstallDir "$LOCALAPPDATA\Programs\Punto de Venta"
InstallDirRegKey HKCU "Software\PuntoDeVenta" "InstallDir"
; Sin administrador: no hace falta y así las actualizaciones son posibles.
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define MUI_ICON "..\iconos\punto-de-venta.ico"
!define MUI_UNICON "..\iconos\punto-de-venta.ico"
!define MUI_ABORTWARNING
ManifestDPIAware true

!define MUI_WELCOMEPAGE_TITLE "Punto de Venta"
!define MUI_WELCOMEPAGE_TEXT "Se instala en tu carpeta (no en Archivos de programa) para que las actualizaciones no pidan administrador.$\r$\n$\r$\nDespués: conecta la impresora y dale a Detectar → Imprimir prueba. El ticket se manda en RAW, sin driver del fabricante."

!define MUI_FINISHPAGE_TITLE "El punto de venta ya está instalado"
!define MUI_FINISHPAGE_TEXT "Conecta la impresora de tickets por USB. Ábrela en Configuración → Impresora → Detectar → Imprimir prueba. No hace falta driver: el programa habla ESC/POS directo. Si Windows la instaló como impresora, elige su nombre (se imprime en modo RAW)."
!define MUI_FINISHPAGE_SHOWREADME "$INSTDIR\manual-cajero.html"
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Abrir el manual del cajero"
!define MUI_FINISHPAGE_RUN "$INSTDIR\pos.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Abrir el punto de venta"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Spanish"

Section "Punto de Venta (obligatorio)"
    SetOutPath "$INSTDIR"
    File /oname=pos.exe "${POS_EXE}"
    File "..\iconos\punto-de-venta.ico"
    File /oname=Guia-impresoras.md "..\..\docs\guia-impresoras.md"
    File /oname=manual-cajero.html "..\..\docs\manual-cajero.html"
    File /oname=Manual-del-cajero.pdf "..\..\docs\Manual-del-cajero.pdf"
    File /oname=cliente_movil.py "..\..\scripts\cliente_movil.py"

    ; Accesos directos
    CreateDirectory "$SMPROGRAMS\Punto de Venta"
    CreateShortcut "$SMPROGRAMS\Punto de Venta\Punto de Venta.lnk" "$INSTDIR\pos.exe" "" "$INSTDIR\punto-de-venta.ico"
    CreateShortcut "$DESKTOP\Punto de Venta.lnk" "$INSTDIR\pos.exe" "" "$INSTDIR\punto-de-venta.ico"
    CreateShortcut "$SMPROGRAMS\Punto de Venta\Manual del cajero.lnk" "$INSTDIR\manual-cajero.html" "" "$INSTDIR\punto-de-venta.ico"
    ; cmd /k deja la ventana abierta: el .exe ya no trae consola por omisión.
    CreateShortcut "$SMPROGRAMS\Punto de Venta\Ver impresoras.lnk" "$SYSDIR\cmd.exe" '/k ""$INSTDIR\pos.exe" --impresoras"' "$INSTDIR\punto-de-venta.ico"
    CreateShortcut "$SMPROGRAMS\Punto de Venta\Probar impresora.lnk" "$SYSDIR\cmd.exe" '/k ""$INSTDIR\pos.exe" --probar-impresion"' "$INSTDIR\punto-de-venta.ico"
    ; Sin ventana: la PC de mostrador atiende al celular del dueño.
    CreateShortcut "$SMPROGRAMS\Punto de Venta\Servir para el celular.lnk" "$SYSDIR\cmd.exe" '/k ""$INSTDIR\pos.exe" --headless"' "$INSTDIR\punto-de-venta.ico"

    ; Los datos (ventas, catálogo, respaldos) viven en la carpeta del usuario,
    ; no junto al programa: si algún día se reinstala, las ventas se quedan.
    CreateDirectory "$LOCALAPPDATA\PuntoDeVenta"
    CreateShortcut "$SMPROGRAMS\Punto de Venta\Carpeta de respaldos.lnk" \
        "$LOCALAPPDATA\PuntoDeVenta" "" "$INSTDIR\punto-de-venta.ico"

    ; La caja atiende al celular por la red (pos --servir). Sin esta regla,
    ; Windows corta la conexión entrante y la app móvil nunca se conecta.
    nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Punto de Venta (LAN)"'
    nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Punto de Venta (LAN)" dir=in action=allow protocol=TCP localport=8080 program="$INSTDIR\pos.exe"'

    ; Desinstalador
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PuntoDeVenta" "DisplayName" "Punto de Venta ${VERSION}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PuntoDeVenta" "UninstallString" "$INSTDIR\desinstalar.exe"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PuntoDeVenta" "DisplayIcon" "$INSTDIR\punto-de-venta.ico"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PuntoDeVenta" "Publisher" "Punto de Venta"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PuntoDeVenta" "DisplayVersion" "${VERSION}"
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PuntoDeVenta" "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PuntoDeVenta" "NoRepair" 1
    WriteRegStr HKCU "Software\PuntoDeVenta" "InstallDir" "$INSTDIR"
    WriteUninstaller "$INSTDIR\desinstalar.exe"
SectionEnd

Section /o "Iniciar con Windows" OPCIONAL
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "PuntoDeVenta" "$INSTDIR\pos.exe"
SectionEnd

LangString DESC_OPCIONAL ${LANG_SPANISH} "Abre el punto de venta automáticamente al encender la computadora."

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${OPCIONAL} $(DESC_OPCIONAL)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
    Delete "$SMPROGRAMS\Punto de Venta\Punto de Venta.lnk"
    Delete "$SMPROGRAMS\Punto de Venta\Carpeta de respaldos.lnk"
    Delete "$SMPROGRAMS\Punto de Venta\Ver impresoras.lnk"
    Delete "$SMPROGRAMS\Punto de Venta\Probar impresora.lnk"
    Delete "$SMPROGRAMS\Punto de Venta\Servir para el celular.lnk"
    Delete "$SMPROGRAMS\Punto de Venta\Manual del cajero.lnk"
    RMDir "$SMPROGRAMS\Punto de Venta"
    Delete "$DESKTOP\Punto de Venta.lnk"

    ; El programa y los restos de una actualización (el ejecutable viejo, el
    ; respaldo, el marcador de "a medias").
    Delete "$INSTDIR\pos.exe"
    Delete "$INSTDIR\pos.exe.anterior"
    Delete "$INSTDIR\pos.exe.nuevo"
    Delete "$INSTDIR\pos.exe.pendiente"
    Delete "$INSTDIR\pos.exe.ultima"
    Delete "$INSTDIR\pos.exe.enuso"
    Delete "$INSTDIR\punto-de-venta.ico"
    Delete "$INSTDIR\Guia-impresoras.md"
    Delete "$INSTDIR\manual-cajero.html"
    Delete "$INSTDIR\Manual-del-cajero.pdf"
    Delete "$INSTDIR\cliente_movil.py"
    Delete "$INSTDIR\desinstalar.exe"
    RMDir "$INSTDIR"

    nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Punto de Venta (LAN)"'
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PuntoDeVenta"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "PuntoDeVenta"

    ; Los datos NO se borran: ahí están las ventas.
    MessageBox MB_OK|MB_ICONINFORMATION "El programa se desinstaló.$\n$\nTus ventas y respaldos quedaron guardados en:$\n$LOCALAPPDATA\PuntoDeVenta"
SectionEnd
