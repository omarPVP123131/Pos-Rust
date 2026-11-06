#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Arma docs/galeria.html con las capturas actuales embebidas (data URI).

Así se ve el estado del programa sin instalarlo: el visor no carga PNG
externos, por eso van dentro del HTML.
"""
from __future__ import annotations

import base64
from pathlib import Path

RAIZ = Path(__file__).resolve().parents[1]
SNAPS = RAIZ / "crates" / "pos-ui" / "tests" / "snapshots"
SALIDA = RAIZ / "docs" / "galeria.html"

PANTALLAS = [
    ("11_ventana_completa.png", "Ventana completa", "Ana (cajero): Venta, Catálogo, Clientes y Caja. Ticket con cantidad × precio. COBRAR F4."),
    ("01_login.png", "Entrar", "Se elige el nombre y se escribe el PIN. Nada pide PIN a media venta."),
    ("02_venta.png", "Venta", "Anaquel, búsqueda y ticket. Baldosas con existencias."),
    ("10_cobro.png", "Cobro", "Total grande, billetes a un toque, cambio en verde."),
    ("03_catalogo.png", "Catálogo", "Alta, existencias y pedidos (el encargado abre pedidos). El cajero sí da de alta un producto."),
    ("04_clientes.png", "Clientes", "Cartera, límite y abonos. «Cliente nuevo» va plegado."),
    ("05_reportes.png", "Reportes", "Ventas, utilidad, devolver y reimprimir. El cajero no entra."),
    ("06_caja.png", "Caja", "Abrir, corte, arqueo y retiros. El cajero corta su caja."),
    ("07_conteo.png", "Conteo", "Conteo físico asistido. Encargado."),
    ("08_auditoria.png", "Bitácora", "Quién hizo qué. Encargado."),
    ("09_configuracion.png", "Configuración", "Negocio, LAN compacta (puerto + URL) y gente. Alta de persona plegada. Sólo administrador."),
    ("12_apariencia.png", "Apariencia", "Tema (de fábrica: Día), letra y color de marca."),
    ("seccion_facturacion.png", "Facturación CFDI 4.0", "Global, individual, notas y complemento de pagos. Por carpetas, sin PAC."),
]


def data_uri(ruta: Path) -> str:
    b64 = base64.b64encode(ruta.read_bytes()).decode("ascii")
    return f"data:image/png;base64,{b64}"


def main() -> None:
    tarjetas = []
    faltan = []
    for archivo, titulo, pie in PANTALLAS:
        ruta = SNAPS / archivo
        if not ruta.is_file():
            faltan.append(archivo)
            continue
        uri = data_uri(ruta)
        tarjetas.append(
            f"""    <figure>
      <figcaption>
        <strong>{titulo}</strong>
        <span>{pie}</span>
      </figcaption>
      <img src="{uri}" alt="{titulo}">
    </figure>"""
        )
    if faltan:
        raise SystemExit("Faltan capturas: " + ", ".join(faltan))

    html = f"""<!DOCTYPE html>
<html lang="es-MX">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Galería — Punto de Venta 0.6.2</title>
<style>
  :root {{
    --fondo: #f4efe6;
    --papel: #fffdf8;
    --tinta: #1c1916;
    --suave: #5c574e;
    --linea: #d9d0c3;
    --verde: #0b6b3a;
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0;
    font-family: "DejaVu Sans", "Segoe UI", system-ui, sans-serif;
    background: var(--fondo);
    color: var(--tinta);
  }}
  header {{
    padding: 28px 32px 18px;
    border-bottom: 3px solid var(--verde);
    background: var(--papel);
  }}
  header h1 {{
    margin: 0 0 6px;
    font-size: 28px;
    letter-spacing: -0.4px;
  }}
  header p {{
    margin: 0;
    color: var(--suave);
    max-width: 62rem;
    line-height: 1.45;
  }}
  .marca {{
    display: inline-block;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--verde);
    margin-bottom: 8px;
  }}
  main {{
    padding: 24px 32px 48px;
    display: grid;
    gap: 28px;
  }}
  figure {{
    margin: 0;
    background: var(--papel);
    border: 1px solid var(--linea);
    border-radius: 14px;
    overflow: hidden;
    box-shadow: 0 8px 24px rgba(28, 25, 22, 0.06);
  }}
  figcaption {{
    padding: 14px 18px 10px;
    border-bottom: 1px solid var(--linea);
  }}
  figcaption strong {{
    display: block;
    font-size: 16px;
    margin-bottom: 4px;
  }}
  figcaption span {{
    color: var(--suave);
    font-size: 13.5px;
    line-height: 1.4;
  }}
  img {{
    display: block;
    width: 100%;
    height: auto;
    background: #eee8dc;
  }}
  footer {{
    padding: 12px 32px 28px;
    color: var(--suave);
    font-size: 12px;
  }}
</style>
</head>
<body>
  <header>
    <div class="marca">Punto de venta 0.6.2</div>
    <h1>Galería del estado actual</h1>
    <p>Capturas reales de la interfaz (1366×768, tema Día). Lo que se ve aquí es lo que corre:
    boutique de mostrador, LAN compacta en Configuración, alta de persona plegada, cajero con
    cuatro pestañas y ticket con cantidad × precio.</p>
  </header>
  <main>
{chr(10).join(tarjetas)}
  </main>
  <footer>Fuente: crates/pos-ui/tests/snapshots/ · regenerar con python3 scripts/armar_galeria.py</footer>
</body>
</html>
"""
    SALIDA.write_text(html, encoding="utf-8")
    print(f"Escrito {SALIDA} ({SALIDA.stat().st_size} bytes, {len(tarjetas)} pantallas)")


if __name__ == "__main__":
    main()
