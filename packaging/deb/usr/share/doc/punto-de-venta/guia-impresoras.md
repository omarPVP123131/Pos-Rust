# Guía de impresoras térmicas

**Cómo conectar cualquier térmica al punto de venta, sin instalar drivers.**

Esta guía es para el día de la instalación. El programa ya trae todo lo que
necesita: no hay que descargar drivers del fabricante, ni instalar CUPS, ni
configurar nada en Windows. Se conecta la impresora, se corre un comando y
listo.

---

## 1. Los tres caminos (y cuál es el tuyo)

| Cómo se conecta | Cómo se ve en la PC | Ruta que se escribe |
|---|---|---|
| **USB** (la mayoría: Epson, Bixolon, Star, y las chinas modernas) | El sistema la reconoce como impresora | `/dev/usb/lp0` |
| **USB que se ve como puerto serie** (las chinas con chip CH340, FTDI, CP210x o Prolific) | Aparece como puerto | `/dev/ttyUSB0` |
| **Red o Wi-Fi** (impresoras de cocina, o la caja y la impresora en lugares distintos) | Tiene IP propia | `192.168.1.50:9100` |
| **Windows** | Nombre de impresora instalada | `POS-80` |

> En Windows no hay rutas: se escribe el **nombre** con el que aparece en
> *Impresoras y escáneres*, y el programa le manda el ticket en modo RAW, que
> es como se imprime ESC/POS sin que Windows lo interprete. Detectar **no**
> toma «Microsoft Print to PDF», Fax ni XPS: no son térmicas.

---

## 2. El instalador ya deja la impresora lista

El `.deb` (Linux) mete al usuario en los grupos `lp` y `dialout`, deja una
regla de `udev` (incluye `usbmisc` para `/dev/usb/lp*`) y recarga las reglas.
El instalador de Windows copia esta guía, abre un atajo *Ver impresoras*
(`pos --impresoras`) y otro *Probar impresora*, y al terminar te dice el
siguiente paso: Detectar → Imprimir prueba, en modo RAW.

Si instalaste con el paquete, **cierra sesión y vuelve a entrar** una vez
para que los grupos se apliquen. Después:

## 2.1 Instalación en tres pasos

```bash
# 1. Conecta y enciende la impresora. Luego pregunta qué hay:
pos --impresoras

# 2. Manda un ticket de prueba (si sale, esa ruta queda guardada):
pos --probar-impresion /dev/usb/lp0

# 3. Si quieres comprobar también el cajón de dinero:
pos --probar-impresion /dev/usb/lp0 --cajon
```

`pos --impresoras` no adivina: **mira** el equipo. Dice la ruta, si es USB o
serie, el fabricante (cuando se puede saber) y el perfil que le conviene:

```
Impresoras encontradas: 2
  Epson (USB /dev/usb/lp0)  [impresora]
      ruta: /dev/usb/lp0
      perfil sugerido: Epson TM (T20/T82/T88) a 115200 baudios
      Impresora USB detectada (clase impresora).
  Adaptador CH340 (serie /dev/ttyUSB0 a 9600 baudios)  [puerto (no confirmado)]
      ruta: /dev/ttyUSB0
      perfil sugerido: Genérica 58 mm a 9600 baudios
      Puerto serie USB. Si es la impresora, revisa los baudios (9600).

Recomendada: /dev/usb/lp0
```

El círculo importa: `●` es una impresora confirmada (clase USB 07) y `○` es un
puerto que *podría* serlo. Si sólo sale `○`, no pasa nada: pruébalo.

---

## 3. El ticket de prueba y cómo leerlo

```
Mi Tienda
PRUEBA DE IMPRESORA
------------------------------------------------
Espanol: á é í ó ú ñ Ñ
Mayusculas: Á É Í Ó Ú
Signos: ¿ ¡ ° º ª $ %
Pagina de codigos: cp850
Puerto: /dev/usb/lp0
------------------------------------------------
Ancho: 48 columnas
123456789012345678901234567890123456789012345678
------------------------------------------------
SI LEE ESTO
EL ANCHO ESTA BIEN
(y este es un QR)
¿Salieron los acentos?
Si salen raros, cambia la
pagina de codigos en Ajustes.
```

Cada parte del ticket existe para responder una pregunta:

* **Los acentos** → ¿la página de códigos es la correcta?
* **La regla de números** → ¿el ancho es el correcto? Si la regla se corta
  antes de llegar al final, el papel es de 58 mm y está configurado como 80.
* **El QR** → ¿la impresora lo soporta? Si sale una lluvia de símbolos, usa el
  perfil «Vieja sin QR».
* **El corte** → ¿tiene cuchilla? Si no corta, usa «Sin cuchilla (58 mm)».

---

## 4. Cuando algo sale mal

### Los acentos salen raros (`jamÃ³n`, o signos en vez de letras)

Es la **página de códigos**. La impresora no entiende UTF-8: recibe un byte por
letra y lo busca en una tabla. Si la tabla no es la correcta, los acentos
salen mal.

Dato verificado en las tablas de Unicode: **CP437, la que traen de fábrica
casi todas las térmicas chinas, no tiene `Á Í Ó Ú`** (le faltan 43 caracteres
del español). CP850 sí los tiene todos. Por eso el programa manda `ESC t 2`
(CP850) al empezar cada ticket.

Si aun así salen mal → *Configuración → Modelo* → prueba con «Genérica 80 mm»
(CP850) o con CP1252 (Windows).

### Sale una hoja con símbolos raros nada más

Son los comandos ESC/POS impresos como texto. Dos causas:

* En **Windows**: el modo RAW no se aplicó. Revisa que el nombre de la
  impresora sea exacto.
* En **serie**: los baudios están mal. Prueba 9600 → 19200 → 38400 → 115200.

### Se imprime la mitad del ticket y se corta

El búfer de la impresora se desbordó. Pasa en los adaptadores serie baratos a
9600 baudios: el ticket llega más rápido de lo que la impresora puede
imprimir. El programa ya manda los bytes por trozos con una pausa calculada
según la velocidad; si aun así se corta, **baja los baudios** a 9600 o sube el
tamaño del trozo bajando la velocidad.

### No imprime nada y dice «Falta permiso»

```bash
sudo usermod -a -G lp $USER       # impresoras USB
sudo usermod -a -G dialout $USER  # adaptadores serie (ttyUSB)
```

Cierra sesión y vuelve a entrar. Si instalaste con el `.deb`, esto ya está
hecho: el instalador mete al usuario en los dos grupos y deja una regla de
udev permanente.

### No imprime y dice «¿Está encendida y conectada?»

El puerto no está. En orden:

1. ¿La impresora está encendida y con papel?
2. ¿El cable está bien? Prueba otro puerto USB.
3. ¿Cambió de ruta? (al reconectar, `ttyUSB0` puede pasar a `ttyUSB1`).
   Corre `pos --impresoras` otra vez. Para evitarlo, usa la ruta fija
   `/dev/serial/by-id/usb-...`, que no cambia.

### Dice que otro programa la está usando

CUPS o el driver del fabricante se quedaron con el puerto. Ciérralos y
vuelve a `pos --probar-impresion`. El punto de venta no necesita CUPS.

### El cajón no abre

* Que el cable RJ11 esté en la impresora (no en la PC).
* Prueba el otro pin: *Configuración → Modelo*. Casi todas abren con el pin 2,
  pero Citizen y algunas Star traen el cajón en el pin 5.
* Compruébalo sin vender: `pos --probar-impresion RUTA --cajon`.

---

## 5. Perfiles incluidos

| Perfil | Cuándo usarlo |
|---|---|
| **Genérica 80 mm** | El de omisión. POS-8022, Xprinter, Zjiang, Gprinter y clones. |
| **Genérica 58 mm** | Las de mostrador chico (POS-5890). 32 columnas. |
| **Sin cuchilla (58 mm)** | Las más baratas: no cortan, se arranca el papel a mano. |
| **Vieja sin QR** | No le mandes QR: lo imprime como basura. |
| **Epson TM (T20/T82/T88)** | Las más comunes en México. Serie a 115200. |
| **Star TSP / mPOP** | Star, 9600 en serie. |
| **Bixolon SRP** | Bixolon, 115200. |
| **Citizen CT-S / CBM** | Citizen: el cajón va en el pin 5. |

Si tu impresora no está en la lista, no importa: cae en **Genérica**, que es
ESC/POS estándar y funciona con la gran mayoría. El perfil se cambia en
*Configuración → Modelo*, y los cambios se guardan en la base de datos (o sea,
se van con el respaldo).

---

## 6. Lo que este programa **no** hace con la impresora

Dicho honestamente, para que no se busque donde no hay:

* **No consulta el estado de la impresora** (si se acabó el papel o está
  abierta la tapa). El comando existe en ESC/POS (`DLE EOT`), pero responder
  depende del modelo y, cuando no responde, deja colgado al programa. Por eso
  no está.
* **No cambia la densidad de impresión ni el logotipo.** Eso son switches o
  utilerías del fabricante.
* **No sirve para impresoras que no hablen ESC/POS** (una láser o una de
  inyección de tinta común).

---

## 7. Para el programador: cómo está hecho

```
crates/pos-hw/src/
  codigo.rs      páginas de códigos (tablas generadas de unicode.org)
  perfil.rs      qué cambia entre modelos: corte, cajón, QR, ancho, velocidad
  transporte.rs  archivo/USB, serie (termios), red (TCP), spooler de Windows
  detectar.rs    descubrimiento leyendo /sys (VID/PID, clase USB)
  escpos.rs      constructor del ticket + ticket de prueba
  receipt.rs     la venta convertida en bytes
```

Todo se prueba **sin impresora**: 61 pruebas en `pos-hw`, incluidas una que
configura un puerto serie de verdad sobre una pseudoterminal (`openpty` +
`termios`, verificando 8N1 y los baudios) y otra que manda un ticket por un
socket real. La detección se prueba contra un árbol falso de `/dev` y `/sys`.

Las tablas de caracteres **no** están escritas a mano: se generaron de los
mapas oficiales de Unicode (`CP437.TXT`, `CP850.TXT`).
