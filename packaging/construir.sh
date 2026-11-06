#!/usr/bin/env bash
# ===========================================================================
#  Arma los instaladores del punto de venta.
#
#  Uso:  ./packaging/construir.sh            # todo lo que se pueda en esta máquina
#        ./packaging/construir.sh deb        # sólo el .deb
#        ./packaging/construir.sh appimage   # AppDir (+ AppImage si hay appimagetool)
#
#  Sale todo en packaging/dist/
# ===========================================================================
set -euo pipefail

VERSION="0.6.2"
RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$RAIZ/packaging/dist"
BUILD="${CARGO_TARGET_DIR:-$RAIZ/target}/release"

# El toolchain puede estar en cualquier parte (en esta arena se instala con
# rustup-init y vive en ~/.cargo). Se busca en los dos sitios conocidos en vez
# de dar por hecho una ruta: si cambia, que el script no se rompa.
export PATH="${PATH}:/var/tmp/posbuild/cargo/bin:$HOME/.cargo/bin"
if [ -d "$HOME/.rustup" ]; then
    export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
else
    export RUSTUP_HOME="${RUSTUP_HOME:-/var/tmp/posbuild/rustup}"
fi
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export CARGO_HOME="${CARGO_HOME:-/var/tmp/posbuild/cargo}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$RAIZ/target}"

que_hacer="${1:-todo}"
mkdir -p "$DIST"

echo "==> Compilando en modo release..."
(cd "$RAIZ" && cargo build --release)

if [ ! -x "$BUILD/pos" ]; then
    echo "!! No encontré el binario en $BUILD/pos" >&2
    exit 1
fi
echo "    binario: $(stat -c%s "$BUILD/pos") bytes"

# ----------------------------------------------------------------- .deb ----
if [ "$que_hacer" = "todo" ] || [ "$que_hacer" = "deb" ]; then
    echo "==> Armando el paquete .deb..."
    DEB="$RAIZ/packaging/deb"
    rm -rf "$DEB/usr/bin" "$DEB/usr/share/doc/punto-de-venta/README"
    mkdir -p "$DEB/usr/bin" "$DEB/usr/share/doc/punto-de-venta"
    cp "$BUILD/pos" "$DEB/usr/bin/pos"
    chmod 755 "$DEB/usr/bin/pos"
    # La licencia va dentro del paquete: Debian la espera en `copyright`.
    if [ -f "$RAIZ/LICENSE" ]; then
        cp "$RAIZ/LICENSE" "$DEB/usr/share/doc/punto-de-venta/copyright"
    fi
    # Los scripts del mantenedor: si el repositorio los perdió, aquí se reponen.
    chmod 0755 "$DEB/DEBIAN"/postinst "$DEB/DEBIAN"/postrm "$DEB/DEBIAN"/prerm 2>/dev/null || true
    # El manual va dentro del paquete: el cajero lo puede leer sin internet.
    if [ -f "$RAIZ/docs/manual-cajero.html" ]; then
        cp "$RAIZ/docs/manual-cajero.html" "$DEB/usr/share/doc/punto-de-venta/manual.html"
    fi
    if [ -f "$RAIZ/docs/Manual-del-cajero.pdf" ]; then
        cp "$RAIZ/docs/Manual-del-cajero.pdf" "$DEB/usr/share/doc/punto-de-venta/Manual-del-cajero.pdf"
    fi
    if [ -f "$RAIZ/docs/guia-impresoras.md" ]; then
        cp "$RAIZ/docs/guia-impresoras.md" "$DEB/usr/share/doc/punto-de-venta/guia-impresoras.md"
    fi
    # El dueño factura y vende desde el celular: este script es la app tonta
    # de la LAN (stdlib de Python, sin dependencias).
    if [ -f "$RAIZ/scripts/cliente_movil.py" ]; then
        cp "$RAIZ/scripts/cliente_movil.py" "$DEB/usr/share/doc/punto-de-venta/cliente_movil.py"
        chmod 755 "$DEB/usr/share/doc/punto-de-venta/cliente_movil.py"
    fi
    # La versión del paquete se escribe aquí: si se queda la de la vez
    # pasada, el .deb dice 0.4.0 en un archivo que se llama 0.5.0.
    sed -i "s/^Version: .*/Version: $VERSION/" "$DEB/DEBIAN/control"
    SALIDA="$DIST/punto-de-venta_${VERSION}_amd64.deb"
    # fakeroot mantiene al dueño como root dentro del paquete sin ser root.
    if command -v fakeroot >/dev/null 2>&1; then
        fakeroot dpkg-deb --build --root-owner-group -Zxz "$DEB" "$SALIDA"
    else
        dpkg-deb --build --root-owner-group -Zxz "$DEB" "$SALIDA"
    fi
    echo "    listo: $SALIDA ($(stat -c%s "$SALIDA") bytes)"
    dpkg-deb -I "$SALIDA" | head -12 || true
fi

# -------------------------------------------------------------- AppImage ----
if [ "$que_hacer" = "todo" ] || [ "$que_hacer" = "appimage" ]; then
    echo "==> Armando el AppImage..."
    APP="$DIST/PuntoDeVenta.AppDir"
    rm -rf "$APP"
    mkdir -p "$APP/usr/bin" "$APP/usr/share/applications" \
             "$APP/usr/share/icons/hicolor/256x256/apps"
    cp "$BUILD/pos" "$APP/usr/bin/pos"
    cp "$RAIZ/packaging/appimage/AppRun" "$APP/AppRun"
    cp "$RAIZ/packaging/appimage/punto-de-venta.desktop" "$APP/usr/share/applications/"
    cp "$RAIZ/packaging/appimage/punto-de-venta.desktop" "$APP/punto-de-venta.desktop"
    cp "$RAIZ/packaging/iconos/256x256.png" "$APP/usr/share/icons/hicolor/256x256/apps/punto-de-venta.png"
    cp "$RAIZ/packaging/iconos/256x256.png" "$APP/punto-de-venta.png"
    # El manual va junto al programa: se abre con doble clic desde la memoria USB.
    [ -f "$RAIZ/docs/Manual-del-cajero.pdf" ] &&
        cp "$RAIZ/docs/Manual-del-cajero.pdf" "$APP/Manual-del-cajero.pdf"
    [ -f "$RAIZ/docs/manual-cajero.html" ] &&
        cp "$RAIZ/docs/manual-cajero.html" "$APP/Manual-del-cajero.html"
    [ -f "$RAIZ/scripts/cliente_movil.py" ] &&
        cp "$RAIZ/scripts/cliente_movil.py" "$APP/cliente_movil.py"
    chmod +x "$APP/AppRun" "$APP/usr/bin/pos"

    if command -v appimagetool >/dev/null 2>&1; then
        ARCH=x86_64 appimagetool "$APP" "$DIST/PuntoDeVenta-${VERSION}-x86_64.AppImage"
        echo "    listo: AppImage generado"
    else
        # Sin appimagetool: se entrega el AppDir y un tar.gz portable que
        # corre igual (descomprimir y ejecutar ./pos).
        (cd "$DIST" && tar czf "PuntoDeVenta-${VERSION}-x86_64-portable.tar.gz" PuntoDeVenta.AppDir)
        echo "    aviso: no hay appimagetool instalado."
        echo "    Se generó el AppDir y un tar.gz portable en su lugar."
        echo "    Para el AppImage real:  wget https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
        echo "                            chmod +x appimagetool-*.AppImage && ./appimagetool-*.AppImage $APP"
    fi
fi

# --------------------------------------------------------------- Windows ----
# No hace falta una máquina con Windows: con el target de mingw instalado y el
# NSIS de Linux, el instalador .exe sale desde aquí mismo.
if [ "$que_hacer" = "todo" ] || [ "$que_hacer" = "windows" ]; then
    echo "==> Windows:"
    if rustup target list --installed 2>/dev/null | grep -q x86_64-pc-windows-gnu \
        && command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
        echo "    Compilando para Windows (x86_64-pc-windows-gnu)..."
        (cd "$RAIZ" && CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
            cargo build --release --target x86_64-pc-windows-gnu)
        WIN="$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release"
        # Si cargo ya compiló ahí mismo, no hay nada que copiar: intentarlo
        # (cp de un archivo a sí mismo) detenía el script antes del instalador.
        DESTINO="$RAIZ/target/x86_64-pc-windows-gnu/release"
        if [ "$WIN" != "$DESTINO" ]; then
            mkdir -p "$DESTINO"
            cp "$WIN/pos.exe" "$DESTINO/"
        fi
        if command -v makensis >/dev/null 2>&1; then
            (cd "$RAIZ/packaging/windows" \
                && makensis -DVERSION="$VERSION" instalador.nsi >/dev/null \
                && mv "PuntoDeVenta-${VERSION}-Instalador.exe" "$DIST/")
            echo "    Instalador: dist/PuntoDeVenta-${VERSION}-Instalador.exe"
        else
            echo "    aviso: no hay makensis (sudo apt-get install -y nsis)."
            echo "    El pos.exe quedó en target/x86_64-pc-windows-gnu/release/"
        fi
    else
        echo "    Falta el toolchain de Windows en esta máquina. Se instala con:"
        echo "      rustup target add x86_64-pc-windows-gnu"
        echo "      sudo apt-get install -y mingw-w64 nsis"
        echo "    Y luego se corre este mismo script."
    fi
fi

# Copia limpia para llevarse: no el AppDir ni restos de compilación.
ENTREGA="${ENTREGA:-$RAIZ/../instaladores}"
mkdir -p "$ENTREGA"
for f in \
    "$DIST/PuntoDeVenta-${VERSION}-Instalador.exe" \
    "$DIST/punto-de-venta_${VERSION}_amd64.deb" \
    "$DIST/PuntoDeVenta-${VERSION}-x86_64.AppImage" \
    "$DIST/PuntoDeVenta-${VERSION}-x86_64-portable.tar.gz"
do
    [ -f "$f" ] && cp -a "$f" "$ENTREGA/"
done
WIN_EXE="$RAIZ/target/x86_64-pc-windows-gnu/release/pos.exe"
[ -f "$WIN_EXE" ] && cp -a "$WIN_EXE" "$ENTREGA/pos.exe"
[ -f "$RAIZ/docs/Manual-del-cajero.pdf" ] && cp -a "$RAIZ/docs/Manual-del-cajero.pdf" "$ENTREGA/"

echo ""
echo "==> Terminado. Artefactos en packaging/dist/:"
ls -lh "$DIST"
echo ""
echo "==> Copia para llevar en $ENTREGA:"
ls -lh "$ENTREGA"
