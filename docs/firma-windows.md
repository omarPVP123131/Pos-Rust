# Firma y distribución en Windows (SmartScreen)

El `.posup` (actualización) ya va firmado con Ed25519 y se verifica antes de
instalar. Eso cubre el **contenido**. Lo que falta para que Windows no
muestre la pantalla azul de "Windows protegió su PC" es firmar el **.exe y el
instalador** con un certificado de firma de código. Son dos problemas
distintos y conviene no confundirlos.

## Hoy (sin certificado, gratis)

1. Arma con `.\packaging\construir-windows.ps1 instalador`.
2. El script deja en `packaging\dist\`:
   - `pos.exe` + `pos-0.6.2-win64-SHA256.txt`
   - `PuntoDeVenta-0.6.2-Instalador.exe` + su `.txt` de SHA256
3. La tienda verifica antes de instalar:
   ```powershell
   (Get-FileHash -Algorithm SHA256 .\pos.exe).Hash.ToLower() -eq (Get-Content .\pos-0.6.2-win64-SHA256.txt).Split()[0]
   ```
4. El `.exe` ya lleva versión embebida (`pos.rc` → Propiedades → Detalles:
   0.6.2, "Punto de Venta - caja para retail"). Si Detalles dice otra
   versión, no es nuestro binario.
5. La primera vez SmartScreen avisa igual (reputación, no firma). El encargado
   pulsa "Más información → Ejecutar de todas formas" **una vez**; con el uso
   la reputación del archivo mejora.

## Cuando haya presupuesto (~USD 100-300/año)

- Compra un certificado de firma de código con validación de organización
  (OV). El EV da reputación inmediata en SmartScreen; el OV la construye con
  el tiempo.
- Firma en CI (nunca subas el `.pfx` al repo; usa Azure Key Vault o similar):
  ```powershell
  azuresigntool sign -kvu https://TU-BOVEDA.vault.azure.net `
    -kvc NOMBRE-DEL-CERT -tr http://timestamp.digicert.com `
    packaging\dist\pos.exe packaging\dist\PuntoDeVenta-0.6.2-Instalador.exe
  ```
- El sellado de tiempo (`-tr`) es obligatorio: sin él, la firma caduca con el
  certificado; con él, vale para siempre.
- Despliegue por etapas como con los `.posup`: 1 tienda piloto → 10 % →
  100 %, con `pos --revertir` como marcha atrás.

## Reglas que no se negocian

- Jamás pidas al usuario desactivar SmartScreen o Defender.
- Cada release sale con su SHA256 publicado por otro canal (WhatsApp del
  negocio, USB del técnico). Si el hash no cuadra, no se instala.
- La llave privada de los `.posup` (`pos-update/llaves/`) sigue separada de
  la de Windows: si una se compromete, la otra sigue valiendo.
