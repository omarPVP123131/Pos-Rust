# Rendimiento medido

Números reales, no impresiones. Todo lo de aquí se puede reproducir.

---

## 1. Lo que se mide y cómo

| Qué | Cómo se mide | Comando |
|---|---|---|
| Consultas de catálogo | 5,000 productos en memoria, 2,000 búsquedas | `cargo test -p pos-db --test rendimiento -- --ignored --nocapture` |
| Lecturas de escáner | 5,000 lecturas de código de barras | idem |
| Búsqueda por folio | 2,000 búsquedas | idem |
| Memoria de la interfaz | `grep VmRSS /proc/<pid>/status` con la app arriba | — |

Las pruebas de rendimiento están marcadas `#[ignore]` a propósito: **miden, no
verifican**, y un número que cambia según la máquina no debe romper la
compilación de nadie.

---

## 2. Números

### Consultas (5,000 productos)

| Operación | Antes | Ahora | Cambio |
|---|---|---|---|
| Lectura de código de barras | 18.6 µs | **4.1 µs** | **4.5× más rápido** |
| Búsqueda por nombre (`LIKE %x%`) | 1.19 ms | 1.15 ms | −3% |
| Búsqueda de venta por folio | — | **0.6 µs** | — |

**Por qué el escáner mejoró tanto:** cada consulta recompilaba su SQL aunque el
texto fuera idéntico siempre. Con `prepare_cached`, SQLite compila una vez y
reutiliza: en una consulta corta eso era casi todo el tiempo.

La búsqueda por nombre apenas mejoró porque ahí el costo es el `LIKE '%texto%'`
sobre 5,000 renglones, no la compilación. Sigue en ~1 ms, que en una tienda de
unos cientos de productos es instantáneo.

### Memoria

| | |
|---|---|
| Memoria real (RSS) con la interfaz abierta | **~125 MB** |
| Pico de memoria virtual | 973 MB |
| Hilos | 9 |
| Tamaño del binario (Linux, release) | **8.0 MB** |

**Sobre el pico de 973 MB:** es memoria **virtual**, no real. Cuenta
reservaciones de direcciones (drivers de video, pilas de hilos), no RAM
ocupada. La que importa es la RSS: 125 MB, con espacio de sobra en 1 GB.

**Advertencia honesta:** estas medidas se tomaron con renderizado por software
(llvmpipe) sobre un X virtual, porque aquí no hay tarjeta de video. En tu
Celeron con su gráfica integrada la memoria puede ser menor (drivers reales) o
la interfaz más fluida; **hay que medirlo en la máquina de verdad** antes de
dar por bueno cualquier número.

### Tamaño del binario

8.0 MB ya con todo puesto: `opt-level="z"`, `lto="fat"`, `codegen-units=1`,
`strip` y `panic="abort"`. No queda nada más que recortar sin quitar una
característica.

---

## 3. Decisiones que se tomaron para que quepa en 1 GB

1. **Rust nativo con egui, sin Electron ni WebView.** Un punto de venta en
   Electron arriba de 300 MB antes de vender el primer producto.
2. **Modo reactivo:** egui sólo dibuja cuando el cajero toca algo. El resto del
   tiempo la CPU está quieta, que en un Celeron se nota en el calor y en el
   ruido del ventilador.
3. **Nada de `f64` para dinero.** Todo son centavos enteros. Más rápido y, sobre
   todo, exacto.
4. **La búsqueda no se dispara con cada tecla** sino al dar Enter (o al
   escanear). Escribir "coca" no son cuatro consultas: es una.
5. **Las respuestas grandes de la API van en caja** (`Box`): cada llamada copia
   menos memoria, y también pesa por la red cuando se usa desde el móvil.

---

## 4. Lo que sigue pendiente

* Medir en el Celeron real (1 GB) con la interfaz de verdad, no en software.
* Probar con la base llena de un año de ventas, no con 5,000 productos de
  prueba: los reportes son la parte que más puede crecer.
* Si la búsqueda por nombre llega a sentirse lenta con el catálogo completo,
  el siguiente paso es FTS5 (índice de texto completo de SQLite), no cambiar de
  base de datos.

---

## 5. Migración 0006: qué se movió en la base

La migración `0006_notas_y_claves.sql` agrega:

* Tres columnas a `products`: `clave_sat`, `clave_unidad`, `unidad_sat`, con
  las genéricas por omisión.
* La tabla `notas_lineas`, que lleva la cuenta de lo que ya se devolvió de cada
  venta, nota por nota. Sin ella no hay manera de saber si dos cocas ya se
  abonaron, y una segunda nota las devolvería otra vez.
* Suelta y vuelve a crear el índice único de facturas para que los egresos
  queden fuera: una venta necesita poder tener **varias** notas parciales, y el
  índice anterior daba por hecho que era una por periodo.
