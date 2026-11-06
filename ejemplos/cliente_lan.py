#!/usr/bin/env python3
"""Cómo habla una app móvil con el punto de venta.

Este archivo es la referencia para quien escriba el cliente Android/iOS:
muestra el contrato completo (login, venta, descuento, fiado, abono, corte)
usando HTTP puro, sin ninguna librería. Está en Python para que se lea fácil,
pero en la app se vería igual con `fetch`, `URLSession` o `http`.

Uso:
    pos --servir --puerto 8080        # en la PC del mostrador
    python3 cliente_lan.py 192.168.1.50 8080
"""

import json
import socket
import sys

HOST = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1"
PUERTO = int(sys.argv[2]) if len(sys.argv) > 2 else 8080


def llamar(comando, token=None):
    """Manda un comando al punto de venta. Devuelve {"Ok": ...} o {"Error": ...}."""
    cuerpo = json.dumps({"token": token, "comando": comando}).encode()
    peticion = (
        b"POST /api HTTP/1.1\r\n"
        b"Host: " + HOST.encode() + b"\r\n"
        b"Content-Type: application/json\r\n"
        b"Content-Length: " + str(len(cuerpo)).encode() + b"\r\n"
        b"Connection: close\r\n\r\n" + cuerpo
    )
    with socket.create_connection((HOST, PUERTO), timeout=5) as s:
        s.sendall(peticion)
        respuesta = b""
        while True:
            trozo = s.recv(65536)
            if not trozo:
                break
            respuesta += trozo
    return json.loads(respuesta.partition(b"\r\n\r\n")[2])


def exigir(respuesta):
    """La app móvil debe tratar el error exactamente igual que la interfaz de PC."""
    if "Error" in respuesta:
        raise SystemExit("El punto de venta dijo: " + respuesta["Error"])
    return respuesta["Ok"]


def main():
    # 1. Quién atiende. La lista de usuarios se pide sin token.
    usuarios = exigir(llamar("Usuarios"))["Usuarios"]
    print("Usuarios:", [u["display_name"] for u in usuarios])

    # 2. Entrar con PIN. El token viaja en cada llamada siguiente.
    sesion = exigir(llamar({"Login": {"username": "admin", "pin": "1234"}}))["Sesion"]
    token = sesion["token"]
    print("Entró:", sesion["usuario"]["display_name"])

    # 3. Abrir caja (sin caja abierta no se puede vender: lo impide el servidor).
    exigir(llamar({"AbrirCaja": {"fondo_cents": 50_000}}, token))

    # 4. Buscar y cobrar. Todo el dinero viaja en centavos (entero).
    producto = exigir(
        llamar({"BuscarProductos": {"termino": "coca", "limite": 5}}, token)
    )["Productos"][0]
    venta = {
        "lineas": [{
            "sku": producto["sku"],
            "descripcion": producto["name"],
            "qty_millis": 3000,          # 3.000 piezas (millonésimas)
            "precio_cents": 1800,        # $18.00
            "descuento_cents": 0,
        }],
        "descuento_global_cents": 0,
        "pagos": [{"metodo": "efectivo", "monto_cents": 5400, "recibido_cents": 6000}],
        "cliente_id": None,
        "nota": "",
        "supervisor": None,
    }
    cobrada = exigir(llamar({"Cobrar": {"venta": venta}}, token))["Venta"]
    print("Venta %s: total $%.2f, cambio $%.2f"
          % (cobrada["folio"], cobrada["total_cents"] / 100, cobrada["cambio_cents"] / 100))

    # 5. Un descuento exige PIN de supervisor: la regla vive en el servidor,
    #    así que la app móvil no puede saltársela aunque quiera.
    con_descuento = json.loads(json.dumps(venta))
    con_descuento["descuento_global_cents"] = 500
    print("Sin supervisor:", llamar({"Cobrar": {"venta": con_descuento}}, token))
    con_descuento["supervisor"] = {"username": "admin", "pin": "1234"}
    exigir(llamar({"Cobrar": {"venta": con_descuento}}, token))

    # 6. Fiado: alta de cliente, venta a crédito y abono.
    id_cliente = int(exigir(llamar({"GuardarCliente": {"cliente": {
        "name": "Doña Mary", "rfc": "", "phone": "5512345678",
        "email": "", "credit_limit_cents": 30_000,
    }}}, token))["Texto"])
    fiado = json.loads(json.dumps(venta))
    fiado["pagos"] = [{"metodo": "credito", "monto_cents": 5400, "recibido_cents": 0}]
    fiado["cliente_id"] = id_cliente
    fiado["supervisor"] = {"username": "admin", "pin": "1234"}
    exigir(llamar({"Cobrar": {"venta": fiado}}, token))
    mary = exigir(llamar({"Clientes": {"termino": "Mary"}}, token))["Clientes"][0]
    print("%s debe $%.2f de $%.2f"
          % (mary["name"], mary["saldo_cents"] / 100, mary["limite_cents"] / 100))
    print("Abono ->", exigir(llamar(
        {"Abonar": {"cliente_id": id_cliente, "monto_cents": 4000, "metodo": "efectivo"}}, token))["Texto"])

    # 7. Corte de caja: el arqueo se manda como (denominación, piezas).
    esperado = exigir(llamar("EstadoCaja", token))["Caja"]["esperado_cents"]
    piezas = [[50_000, 1], [10_000, 1], [200, 40]]      # $500 + $100 + $8
    contado = sum(d * c for d, c in piezas)
    exigir(llamar({"CerrarCaja": {
        "piezas": piezas,
        # Si el arqueo no cuadra, hay que mandar supervisor.
        "supervisor": None if contado == esperado else {"username": "admin", "pin": "1234"},
    }}, token))
    print("Caja cerrada. Esperado $%.2f, contado $%.2f" % (esperado / 100, contado / 100))


if __name__ == "__main__":
    main()
