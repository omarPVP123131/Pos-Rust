#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Cliente de la LAN: lo que haría la app móvil del dueño.

Habla con el punto de venta por HTTP, **sin lógica de negocio**. El celular
(o este script) sólo pide y muestra: cobrar, ver el día y facturar un ticket
viven en el servidor (`pos --headless`).

Sin dependencias: sólo la biblioteca estándar.

Uso:
  python3 cliente_movil.py http://127.0.0.1:8080
  python3 cliente_movil.py http://192.168.1.50:8080 --usuario admin --pin 1234
  python3 cliente_movil.py http://127.0.0.1:8080 --completo

El servidor:
  pos --headless --db /tmp/tienda.db --puerto 8080
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request
from datetime import date
from typing import Any

HOY = date.today().isoformat()


class Pos:
    def __init__(self, base: str, timeout: float = 8.0) -> None:
        self.base = base.rstrip("/")
        self.timeout = timeout
        self.token: str | None = None

    def salud(self) -> dict[str, Any]:
        req = urllib.request.Request(self.base + "/api/salud", method="GET")
        with urllib.request.urlopen(req, timeout=self.timeout) as r:
            return json.loads(r.read().decode("utf-8"))

    def pedir(self, comando: Any) -> Any:
        cuerpo = json.dumps({"token": self.token, "comando": comando}).encode("utf-8")
        req = urllib.request.Request(
            self.base + "/api",
            data=cuerpo,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as r:
                raw = r.read().decode("utf-8")
        except urllib.error.HTTPError as e:
            raw = e.read().decode("utf-8", "replace")
        except urllib.error.URLError as e:
            raise SystemExit(f"No se pudo conectar a {self.base}: {e.reason}") from e
        data = json.loads(raw)
        if "Error" in data:
            raise RuntimeError(data["Error"])
        if "Ok" not in data:
            raise RuntimeError(f"Respuesta rara: {data}")
        return data["Ok"]

    def unwrap(self, cuerpo: Any, etiqueta: str) -> Any:
        if not isinstance(cuerpo, dict) or etiqueta not in cuerpo:
            raise RuntimeError(f"Se esperaba {etiqueta}, llegó {cuerpo!r}")
        return cuerpo[etiqueta]


def pesos(cents: int) -> str:
    signo = "-" if cents < 0 else ""
    n = abs(cents)
    return f"{signo}${n / 100:.2f}"


def login(pos: Pos, usuario: str, pin: str) -> str:
    pos.token = None
    gente = pos.unwrap(pos.pedir("UsuariosLogin"), "Usuarios")
    nombres = ", ".join(u.get("display_name") or u.get("username") for u in gente)
    print(f"  Quién puede entrar: {nombres}")
    sesion = pos.unwrap(
        pos.pedir({"Login": {"username": usuario, "pin": pin}}), "Sesion"
    )
    pos.token = sesion["token"]
    u = sesion["usuario"]
    print(f"  Entró {u['display_name']} ({u['username']}, {u['role']})")
    return pos.token or ""


def asegurar_caja(pos: Pos, fondo_cents: int = 50_000) -> None:
    caja = pos.unwrap(pos.pedir("EstadoCaja"), "Caja")
    if caja.get("abierta"):
        print(f"  Caja abierta. Esperado {pesos(caja['esperado_cents'])}")
        return
    pos.pedir({"AbrirCaja": {"fondo_cents": fondo_cents}})
    print(f"  Caja abierta con fondo {pesos(fondo_cents)}")


def cliente_con_rfc(pos: Pos) -> int:
    """Alta (o reuso) de un cliente al que sí se le puede facturar el ticket."""
    existentes = pos.unwrap(
        pos.pedir({"Clientes": {"termino": "ROQJ800101ABC"}}), "Clientes"
    )
    for c in existentes:
        if (c.get("rfc") or "").upper() == "ROQJ800101ABC":
            print(f"  Cliente {c['name']} (id {c['id']}) ya tenía RFC")
            return int(c["id"])
    texto = pos.unwrap(
        pos.pedir(
            {
                "GuardarCliente": {
                    "cliente": {
                        "name": "Juan Pérez",
                        "rfc": "ROQJ800101ABC",
                        "phone": "5512345678",
                        "email": "juan@ejemplo.mx",
                        "credit_limit_cents": 0,
                        "regimen_fiscal": "612",
                        "codigo_postal": "01000",
                        "uso_cfdi": "G03",
                    }
                }
            }
        ),
        "Texto",
    )
    cid = int(texto)
    print(f"  Cliente Juan Pérez dado de alta (id {cid}) con RFC")
    return cid


def vender(
    pos: Pos,
    sku: str,
    qty: int = 1,
    efectivo_extra: int = 0,
    cliente_id: int | None = None,
) -> dict[str, Any]:
    prods = pos.unwrap(
        pos.pedir({"BuscarProductos": {"termino": sku, "limite": 5}}), "Productos"
    )
    if not prods:
        raise RuntimeError(f"No encontré {sku}")
    p = prods[0]
    total = p["price_cents"] * qty
    recibido = total + efectivo_extra
    venta = pos.unwrap(
        pos.pedir(
            {
                "Cobrar": {
                    "venta": {
                        "lineas": [
                            {
                                "sku": p["sku"],
                                "descripcion": p["name"],
                                "qty_millis": qty * 1000,
                                "precio_cents": p["price_cents"],
                                "descuento_cents": 0,
                            }
                        ],
                        "descuento_global_cents": 0,
                        "pagos": [
                            {
                                "metodo": "efectivo",
                                "monto_cents": total,
                                "recibido_cents": recibido,
                            }
                        ],
                        "cliente_id": cliente_id,
                        "nota": "desde el celular",
                        "supervisor": None,
                    }
                }
            }
        ),
        "Venta",
    )
    print(
        f"  Venta {venta['folio']}: {p['name']} × {qty} = {pesos(venta['total_cents'])}"
        f"  cambio {pesos(venta['cambio_cents'])}"
    )
    return venta


def preparar_fiscal(pos: Pos) -> None:
    cfg = pos.unwrap(pos.pedir("ConfigFiscal"), "ConfigFiscal")
    if cfg.get("rfc"):
        return
    pos.pedir(
        {
            "GuardarConfigFiscal": {
                "config": {
                    "rfc": "XAXX010101000",
                    "nombre": "TIENDA DE MOSTRADOR",
                    "regimen_fiscal": "626",
                    "codigo_postal": "01000",
                    "forma_pago": "01",
                    "periodicidad": "01",
                    "carpeta_salida": "",
                    "carpeta_entrada": "",
                }
            }
        }
    )
    print("  Datos fiscales de demostración guardados (RFC genérico).")


def facturar_ticket(pos: Pos, folio: str) -> dict[str, Any]:
    preparar_fiscal(pos)
    f = pos.unwrap(
        pos.pedir({"FacturarVenta": {"folio": folio, "fecha": HOY}}), "Factura"
    )
    print(
        f"  Factura {f['folio']} de la venta {folio}: {pesos(f['total_cents'])} ({f['estado']})"
    )
    return f


def mostrar_dia(pos: Pos) -> None:
    t = pos.unwrap(pos.pedir("TotalesHoy"), "Totales")
    print(
        f"  Hoy: {t['count']} tickets, vendido {pesos(t['total_cents'])}, "
        f"utilidad {pesos(t.get('utilidad_cents', 0))}"
    )
    util = pos.pedir({"UtilidadHoy": {"limite": 5}})
    filas = util.get("Utilidad") or []
    for u in filas[:5]:
        print(
            f"    {u['nombre']}: vendido {pesos(u['vendido_cents'])}  "
            f"margen {pesos(u['utilidad_cents'])}"
        )


def dia_del_dueno(pos: Pos, usuario: str, pin: str) -> None:
    """Lo que el dueño hace desde el celular: mirar, vender, facturar."""
    print("1. ¿El servidor responde?")
    s = pos.salud()
    print(f"  salud: {s}")

    print("2. Entrar")
    login(pos, usuario, pin)

    print("3. Cómo va el día")
    mostrar_dia(pos)

    print("4. Abrir caja si hace falta (sin caja no se cobra)")
    asegurar_caja(pos)

    print("5. Cliente con RFC (sin RFC el ticket va a la factura global)")
    cid = cliente_con_rfc(pos)

    print("6. Cobrar una Coca a ese cliente desde el celular")
    venta = vender(pos, "7501", qty=2, efectivo_extra=200, cliente_id=cid)

    print("7. El día, otra vez")
    mostrar_dia(pos)

    print("8. Facturar ese ticket (CFDI 4.0, queda en la cola / carpeta)")
    try:
        facturar_ticket(pos, venta["folio"])
    except RuntimeError as e:
        print(f"  No se facturó: {e}")

    print("9. Últimas ventas")
    recientes = pos.unwrap(pos.pedir({"VentasRecientes": {"limite": 5}}), "Ventas")
    for v in recientes:
        print(f"    {v['folio']}  {pesos(v['total_cents'])}  {v['status']}")


def completo(pos: Pos, usuario: str, pin: str) -> int:
    """Recorre la API como prueba de humo. Sale 0 si todo respondió."""
    fallos: list[str] = []

    def paso(nombre: str, fn) -> None:
        try:
            fn()
            print(f"  OK  {nombre}")
        except Exception as e:  # noqa: BLE001 — queremos ver cualquier fallo
            fallos.append(f"{nombre}: {e}")
            print(f"  FALLO  {nombre}: {e}")

    print("Comprobación completa contra", pos.base)
    paso("salud", lambda: pos.salud())
    paso("login", lambda: login(pos, usuario, pin))
    paso("caja", lambda: asegurar_caja(pos))
    paso("buscar", lambda: pos.pedir({"BuscarProductos": {"termino": "coca", "limite": 3}}))
    paso(
        "codigo de barras",
        lambda: pos.pedir({"ProductoPorCodigo": {"codigo": "750100000"}}),
    )
    venta_folio = {"folio": ""}
    publico = {"folio": ""}

    def _publico() -> None:
        v = vender(pos, "7502", qty=1, efectivo_extra=150)
        publico["folio"] = v["folio"]

    def _vender_rfc() -> None:
        cid = cliente_con_rfc(pos)
        v = vender(pos, "7501", qty=1, efectivo_extra=200, cliente_id=cid)
        venta_folio["folio"] = v["folio"]

    paso("cobrar público", _publico)
    paso("cobrar a cliente con RFC", _vender_rfc)
    paso("totales", lambda: mostrar_dia(pos))
    paso("codigo propio", lambda: print("   ", pos.pedir("CodigoPropio")))
    paso("clientes", lambda: pos.pedir({"Clientes": {"termino": ""}}))
    paso("categorias", lambda: pos.pedir("Categorias"))

    def _pedido() -> None:
        pid = int(
            pos.unwrap(
                pos.pedir(
                    {
                        "GuardarProveedor": {
                            "proveedor": {
                                "id": None,
                                "name": "Bebidas del Valle",
                                "phone": "555",
                                "nota": "",
                            }
                        }
                    }
                ),
                "Texto",
            )
        )
        pedido = pos.unwrap(
            pos.pedir({"NuevoPedido": {"proveedor_id": pid, "nota": ""}}), "Pedido"
        )
        pedido = pos.unwrap(
            pos.pedir(
                {
                    "AgregarAPedido": {
                        "pedido_id": pedido["id"],
                        "codigo": "7503",
                        "qty_millis": 2000,
                        "costo_cents": 2000,
                    }
                }
            ),
            "Pedido",
        )
        prod = pedido["lineas"][0]["product_id"]
        pedido = pos.unwrap(
            pos.pedir(
                {
                    "RecibirPedido": {
                        "id": pedido["id"],
                        "entregas": [{"product_id": prod, "qty_millis": 2000}],
                    }
                }
            ),
            "Pedido",
        )
        print(f"  Pedido {pedido['folio']} {pedido['estado']}")

    paso("pedido a proveedor", _pedido)
    paso("fiscal", lambda: preparar_fiscal(pos))
    paso(
        "facturar ticket",
        lambda: facturar_ticket(pos, venta_folio["folio"])
        if venta_folio["folio"]
        else (_ for _ in ()).throw(RuntimeError("no hubo venta")),
    )
    paso(
        "factura global",
        lambda: print("   ", pos.pedir({"FacturaGlobal": {"fecha": HOY, "periodicidad": "01"}})),
    )
    paso("cola de facturas", lambda: pos.pedir({"Facturas": {"limite": 10}}))
    paso("impresoras", lambda: pos.pedir("Impresoras"))
    paso("estado actualización", lambda: pos.pedir("EstadoActualizacion"))
    paso("respaldos", lambda: pos.pedir("Respaldos"))
    if fallos:
        print(f"\n{len(fallos)} fallo(s):")
        for f in fallos:
            print(" -", f)
        return 1
    print("\nTodo respondió. El celular y el mostrador hablan el mismo contrato.")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(
        description="Cliente tonto de la LAN (como la app móvil del dueño)."
    )
    p.add_argument("url", help="http://IP:PUERTO del pos --headless")
    p.add_argument("--usuario", default="admin")
    p.add_argument("--pin", default="1234")
    p.add_argument(
        "--completo",
        action="store_true",
        help="recorre la API (prueba de humo) y sale con código distinto de 0 si algo falla",
    )
    args = p.parse_args()
    pos = Pos(args.url)
    if args.completo:
        return completo(pos, args.usuario, args.pin)
    dia_del_dueno(pos, args.usuario, args.pin)
    return 0


if __name__ == "__main__":
    sys.exit(main())
