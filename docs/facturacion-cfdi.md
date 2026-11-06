# Facturación electrónica (CFDI 4.0)

Estado: **factura global, factura individual a clientes con RFC, notas de
crédito —total y parcial— y pantalla están listos y probados. Falta el cliente
HTTP de un PAC concreto** (se timbra por carpetas mientras tanto). Este
documento explica qué hace, cómo se usa y qué le falta.

**Dónde se usa:** botón *Config* (F10) → sección **Facturación electrónica
(CFDI 4.0)**. Ahí se llenan los datos fiscales una vez, se genera la factura
global del día y se ve la cola con su estado.

---

## 1. Qué hace hoy

### Arma el comprobante

`pos-fiscal` fabrica el XML de un CFDI 4.0 completo, con lo que el Anexo 20 pide:

| Nodo | Atributos que se llenan |
|---|---|
| `Comprobante` | `Version="4.0"`, `Serie`, `Folio`, `Fecha`, `FormaPago`, `Moneda="MXN"`, `SubTotal`, `Descuento`, `Total`, `TipoDeComprobante="I"`, `Exportacion="01"`, `MetodoPago`, `LugarExpedicion` |
| `Comprobante` (sólo global) | `Periodicidad`, `Meses`, `Año` |
| `Emisor` | `Rfc`, `Nombre`, `RegimenFiscal` |
| `Receptor` | `Rfc`, `Nombre`, `DomicilioFiscalReceptor`, `RegimenFiscalReceptor`, `UsoCFDI` |
| `Concepto` | `ClaveProdServ`, `NoIdentificacion`, `Cantidad`, `ClaveUnidad`, `Unidad`, `Descripcion`, `ValorUnitario`, `Importe`, `Descuento`, `ObjetoImp` |
| `Impuestos` | `TotalImpuestosTrasladados` y el `Traslado` de IVA (impuesto `002`) |

En moneda nacional **no** se manda `TipoCambio`: el SAT lo rechaza.

### La factura global

Es la que usa el 95% de las tiendas: nadie pide factura en el mostrador, y al
cerrar se emite una sola por todo el día a nombre del público en general
(RFC `XAXX010101000`, uso `S01`, régimen `616`).

`factura_global()` junta las ventas del día, separa por tasa (un ticket con
cosas al 16% y cosas exentas produce dos renglones), y genera el comprobante.

### Los centavos cuadran

Esta es la parte que más trabajo costó y la que más vale:

* Los precios del sistema **traen el IVA incluido**, como se vende en México.
* El SAT pide el importe **sin** IVA y el IVA aparte.
* El IVA se calcula **por complemento**: `neto + iva == importe cobrado`,
  siempre, sin excepción. Se probó con los primeros 20,000 importes.

Si la factura dice $4,318.27 y los tickets del día suman $4,318.28, el contador
pierde la tarde. Aquí eso no puede pasar.

### La cola

La factura **se genera y se guarda siempre**, se timbre o no:

```
pendiente  →  (reintentar)  →  timbrada   (con UUID y XML del PAC)
                           →  error       (sólo si el PAC la rechazó)
```

* Si el PAC no contesta, la factura **sigue pendiente** y se reintenta.
* Si el PAC la rechaza, se marca como `error` con el motivo: reintentar un
  comprobante mal hecho sólo gasta tiempo.
* Las ventas incluidas se marcan para no facturarse dos veces.
* Una sola factura global por periodo: darle dos veces al botón no duplica.

---

## 2. Cómo se usa

### Desde la interfaz

`Config` (F10) → **Facturación electrónica (CFDI 4.0)**.

1. **Datos fiscales** (una sola vez): RFC, nombre o razón social, régimen
   fiscal, código postal, forma de pago y periodicidad de la factura global.
   Todo sale de la constancia de situación fiscal.
2. **Carpetas del PAC** (opcional): dónde dejar los XML por timbrar y de dónde
   recoger los timbrados.
3. **Generar factura global**: se elige el día y la periodicidad, y el
   comprobante queda en la cola.
4. **La cola** muestra cada factura con su estado —*Pendiente* (ámbar),
   *Timbrada* (verde, con su UUID) o *Rechazada* (rojo, con el motivo)— y los
   botones *Reintentar* y *XML* para ver el comprobante.

### Desde la API

Los mismos comandos, que son los que consume también la app móvil:

| Comando | Qué hace |
|---|---|
| `ConfigFiscal` | Lee los datos fiscales de la tienda |
| `GuardarConfigFiscal` | Los guarda (RFC, razón social, régimen, CP, forma de pago, periodicidad, carpetas) |
| `FacturaGlobal { fecha, periodicidad }` | Genera la factura global de un día |
| `Facturas { limite }` | Lista la cola |
| `ReintentarFactura { id }` | Vuelve a intentar el timbrado |
| `XmlFactura { id }` | Entrega el XML (el timbrado si ya se timbró) |

Antes de generar nada, el sistema revisa lo que se puede revisar y lo dice en
palabras de la tienda: *"El RFC debe tener 12 o 13 caracteres"*, no un código
de error.

---

## 3. Notas de crédito (devoluciones)

Una devolución en la caja ya existía; lo que faltaba era el comprobante que le
dice al SAT "de aquella factura, quítale esto". Eso es un **CFDI de egresos**,
tipo `E`.

Cómo se hace: `Config` → **Facturación electrónica** → *Nota de crédito* → se
escribe el folio, se pulsa **Buscar venta** y se anota cuánto se devuelve de
cada renglón (vacío o cero = ese no). Si todos quedan en cero, la nota cubre la
venta completa.

Lo importante, y la razón por la que no se puede hacer de otra manera:

* La nota va **ligada por UUID** a la factura que la originó
  (`CfdiRelacionados` con `TipoRelacion="01"`). Sin ese vínculo el SAT no sabe
  de qué se está restando y la nota no vale.
* Por eso **si la venta no se ha facturado, no hay nota**. El sistema lo dice
  con todas sus letras ("todavía no está facturada") en vez de fabricar un
  comprobante huérfano que el PAC rechazaría.
* Un egreso **no** lleva `FormaPago` ni `MetodoPago`: no entra dinero, y el SAT
  los rechaza ahí.
* **Una venta puede tener varias notas parciales**, hasta completar lo que se
  vendió: devuelves dos cosas hoy y tres mañana, y el sistema lleva la cuenta
  de lo que ya se abonó. Pedir devolver más de lo que queda da un error con el
  número exacto ("Sólo quedan 3 por devolver"), porque devolver de más no es
  una devolución: es regalar dinero con una factura encima.

### Detalle que importa: por qué la cantidad del renglón siempre es 1

En la nota, cada renglón trae cantidad 1 y la cantidad real va en la
descripción ("2 x Coca-Cola 600 ml"). No es flojera: un precio con IVA incluido
no reparte exacto entre piezas ($15.52 × 2 = $31.04, no $31.03). Si se pusiera
la cantidad como viene, la nota diría un importe distinto al dinero que de
veras se devolvió — el mismo bug del centavo que ya se corrigió una vez.

### Lo que todavía no hace

* **Clientes con RFC propio**: hoy todas las facturas son al público en
  general. Para hacerle nota de crédito a un cliente con RFC hay que guardar
  sus datos fiscales completos (régimen, código postal, uso CFDI), y eso es lo
  siguiente.

---

## 4. Factura individual: cuando el cliente te da su RFC

Hay dos maneras de facturar en México y no se pueden mezclar:

* **Factura global**: una al día (o al mes) que ampara todo lo que se vendió a
  público en general. El SAT la permite porque nadie pidió factura.
* **Factura individual**: una por venta, a nombre de quien la pidió.

**Una venta a nombre de alguien con RFC no puede meterse en la global.** Si se
metiera, el SAT vería el mismo ingreso dos veces, y eso se descubre hasta que
el contador te habla en marzo.

Cómo funciona aquí:

1. Al **cobrar**, si la venta es de un cliente que tiene RFC, régimen fiscal y
   código postal completos, queda marcada como `requerida`. La global del día
   la deja fuera automáticamente.
2. En `Config` → *Factura de una venta* escribes el folio y la factura sale a
   nombre del cliente, con su RFC, su régimen, su código postal y su uso CFDI.
3. Si le falta cualquier dato, el sistema dice **exactamente cuál** en vez de
   fallar con un error críptico.

### Devolver por monto

Además de piezas, se puede devolver dinero: *"te devuelvo $50"*. El monto se
reparte entre los renglones en proporción, con reparto por residuo mayor para
que la suma sea exacta. Si las piezas no reparten exacto, el programa lo dice
en vez de fallar: la nota ya está hecha.

### El aviso que importa

Como esas ventas no entran en la global, **si nadie las factura, ese ingreso se
queda sin reportar**. La pantalla lo dice con el número de ventas esperando:
*"Hay 3 ventas de clientes con RFC esperando factura"*.

### Lo que todavía no hace

* **Cancelación con folio fiscal** (pedirle al PAC que cancele y recibir el
  acuse). Hoy se corrige con nota de crédito, que es lo que procede cuando la
  venta ya se cobró.

---

## 5. Complemento de pagos: cuando el cliente paga después

Si le vendes a crédito a un restaurante, la factura sale con
`MetodoPago="PPD"` (pago en parcialidades o diferido). Esa factura **se queda
debiendo**: el cliente ya la puede deducir, pero el SAT todavía no sabe nada
del dinero.

Cada vez que ese cliente te paga, se emite un **complemento de pagos**: un
CFDI aparte, tipo `P`, que va ligado por UUID a las facturas que está
cubriendo. Sin ese comprobante, la factura PPD está incompleta y a quien la
recibe se la pueden rechazar.

### Cómo se ve por dentro

```xml
<pago20:Pago FechaPago="2026-10-20T12:00:00" FormaDePagoP="03"
             MonedaP="MXN" Monto="2500.00">
  <pago20:DoctoRelacionado IdDocumento="FACTURA-CREDITO-1" MonedaDR="MXN"
      NumParcialidad="1" ImpSaldoAnt="5000.00" ImpPagado="2500.00"
      ImpSaldoInsoluto="2500.00" ObjetoImpDR="02">
    <pago20:ImpuestosDR>
      <pago20:TrasladosDR>
        <pago20:TrasladoDR BaseDR="2155.17" ImpuestoDR="002"
            TipoFactorDR="Tasa" TasaOCuotaDR="0.160000" ImporteDR="344.83"/>
```

### Los tres números que se piden siempre

| Atributo | Qué dice |
|---|---|
| `ImpSaldoAnt` | Cuánto debía la factura **antes** de este pago. |
| `ImpPagado` | Cuánto se le está aplicando a esa factura. |
| `ImpSaldoInsoluto` | Cuánto queda debiendo después. |

En la primera parcialidad, `ImpSaldoAnt` es el total de la factura. En la
segunda, es lo que quedó de la primera. El programa los saca del saldo
guardado, no los vuelve a calcular.

### El IVA se reporta proporcional

Éste es el detalle que más se equivoca: **no se reporta el IVA completo de la
factura, sino la parte que corresponde a lo que se pagó**.

De una factura de $6,475.00 (con $896.55 de IVA) pagan $5,000.00. El IVA que
va en el complemento es **$692.32**, no $896.55:

```
IVA del pago = 5,000.00 × (896.55 / 6,475.00) = 692.32
```

Y la base más el impuesto dan exactamente el pago: $4,307.68 + $692.32 =
$5,000.00. Ni un centavo de más ni de menos.

El resumen (`BaseP` / `ImporteP`, dentro de `ImpuestosP`) es la suma de los
documentos de ese pago, y los `Totales` la suma de todos los pagos del
comprobante: el SAT valida que cuadren.

### `ObjetoImpDR`: 02 con impuestos, 01 sin ellos

No es decorativo. Si el documento relacionado lleva `ImpuestosDR`, el
`ObjetoImpDR` tiene que ser **"02"** (sí objeto de impuesto); si va "01", el
nodo de impuestos **no debe existir** y el PAC lo rechaza. El programa lo
pone según el caso, y hay una prueba para cada uno.

### Cómo se usa

En Configuración → Facturación electrónica, abajo de las notas de crédito:

1. Eliges al cliente (sólo aparecen los que tienen RFC: sin RFC no hay
   factura a su nombre, así que tampoco hay complemento).
2. Picando **"Cuánto debe"** sale el saldo de sus facturas a crédito.
3. Anotas cuánto pagó, el día y cómo pagó, y **"Registrar pago"**.

El pago se aplica a las facturas pendientes **de la más vieja a la más
nueva**, que es como se hace normalmente. Si el dinero alcanza para varias,
el programa las va cubriendo en orden y a cada una le pone su número de
parcialidad. Si sobra dinero, te lo dice antes de emitir nada.

Cuando la factura queda en saldo cero, ya no acepta más pagos.

---

## 6. El PAC: cómo se timbra

El SAT no recibe comprobantes directos: hay que pasar por un PAC. Cada PAC
tiene su API, así que esto se modela como un **trait**:

```rust
pub trait Pac {
    fn nombre(&self) -> &str;
    fn timbrar(&self, xml: &str) -> Result<Timbre, ErrorPac>;
}
```

### Lo que ya está: timbrado por carpetas

`PacCarpeta` no necesita internet ni credenciales:

1. El programa deja el XML en la carpeta de salida, con el **folio** por nombre
   (`G20261011-01.xml`) para que el contador lo encuentre.
2. Alguien (el contador con su sistema, o un PAC en automático) pone el XML
   timbrado en la carpeta de entrada.
3. El programa lo recoge, lee el UUID del `TimbreFiscalDigital` y cierra el ciclo.

Es exactamente como trabajan muchos despachos con clientes chicos, y permite
probar todo el camino sin gastar un solo timbre.

### Lo que falta: el cliente HTTP

Para timbrar en automático hay que implementar `Pac` contra la API del PAC
que se contrate (Facturapi, SW Sapien, Diverza, Finkok...). **No se hizo a
ciegas por dos razones:**

1. No se puede probar sin credenciales reales de un contribuyente real, y
   mandar XML a un PAC de producción "para ver qué pasa" gasta timbres.
2. Timbrar por HTTPS obliga a meter una librería de TLS, y éste es un programa
   que tiene que caber cómodo en 1 GB de RAM. La opción que conviene es
   `native-tls`, que usa el almacén del sistema (Schannel en Windows, OpenSSL
   en Linux) en vez de cargar su propia criptografía.

En cuanto se elija el PAC, se implementa el trait y no hay que tocar nada más:
ni la base, ni la cola, ni la interfaz.

### El sello

El `Sello` va vacío a menos que se configure un CSD propio. Es lo normal
cuando el PAC firma con los sellos del contribuyente (multi-CSD).

---

## 7. Lo que esto NO es

* **No valida contra el XSD del SAT.** Genera el comprobante correcto para el
  caso más común (ingreso simple y factura global al público en general).
* **No sustituye a un contador.** Las claves `ClaveProdServ` y `ClaveUnidad`
  están puestas en las genéricas (`01010101` y `H87`); para un negocio con
  obligaciones específicas hay que afinarlas por producto.
* **No emite** CFDI de egresos (notas de crédito), nómina, ni complemento de
  pagos (REP). Esos son los siguientes pasos naturales.

---

## 8. Pruebas

42 pruebas en `pos-fiscal`, 16 de facturación en `pos-api` y 15 de interfaz en
`pos-ui`:

* El XML está bien formado (revisor con pila de etiquetas).
* Los caractéres especiales no lo rompen: `Pan & Dulce <1 kg>` se escapa bien.
* Los acentos se conservan (UTF-8).
* El total cuadra: `subtotal − descuento + IVA == total`.
* El IVA nunca pierde centavos en 20,000 importes distintos.
* La factura global suma exactamente lo que se cobró ese día.
* No se cuelan ventas de otro día, ni se facturan dos veces.
* El ciclo completo por carpetas: se deja el XML, se recoge el timbrado, las
  ventas quedan marcadas.
* Una tasa desconocida se trata como exenta en vez de inventar un IVA.
* La nota de crédito trae el UUID de la factura original y no lleva forma de pago.
* Una nota parcial cubre dos de cinco cosas, y una segunda cubre las tres que
  quedan; la tercera ya no pasa.
* Pedir devolver más de lo vendido da error con el número exacto.
* Una venta de cliente con RFC queda fuera de la global y se factura aparte,
  con su RFC, régimen, código postal y uso CFDI en el XML.
* No se puede facturar dos veces la misma venta.
* **Regresión del centavo:** ninguna combinación de importes (fuerza bruta sobre
  400 importes × 4 mezclas) pierde un centavo al juntar renglones.
* El complemento es tipo `P`, sin importe, con `Moneda="XXX"` y el concepto
  fijo `84111506`.
* Pagar la mitad de una factura reporta **la mitad de su IVA**, no el completo,
  y base + impuesto dan el pago exacto.
* Con impuestos el documento va `ObjetoImpDR="02"` y el pago trae su resumen;
  sin impuestos va `"01"` y no hay nodo de impuestos.
* Cobrar en dos exhibiciones: la primera deja saldo, la segunda lo deja en
  cero y queda marcada como parcialidad 2.
* Pagar de más se rechaza antes de emitir nada; pagar de a $0 también.

---

## 9. Siguientes pasos

1. **Cliente HTTP del PAC** que se contrate (el de carpetas ya funciona).
2. **Cancelación con folio fiscal** (pedirle al PAC que cancele y recibir el
   acuse).

---

## 10. Un bug que se encontró (y por qué quedó documentado)

Vendiendo 2 × $18.00 + 1 × $18.50, la factura salía por **$54.49** en vez de
$54.50. Se perdía un centavo al juntar renglones del mismo IVA: el importe del
impuesto se recalculaba como `base × 16%` en vez de usar el complemento que
completa lo cobrado.

La causa de fondo es que **un precio con IVA incluido no reparte exacto**:
$36.00 entre 1.16 da 31.0344…, y no hay centavos enteros que cuadren
($31.03 + $4.96 = $35.99; $31.04 + $4.97 = $36.01). Alguien tiene que quedarse
con ese centavo, y la decisión fue que **la factura siempre diga lo que la caja
cobró**: el IVA se calcula por complemento y se guarda dicho con número, no
recetado.

Se agregó una prueba de fuerza bruta para que no vuelva a pasar.
