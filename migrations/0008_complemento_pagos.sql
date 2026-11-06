-- Complemento de pagos (CFDI tipo P).

-- Una factura PPD (pago diferido) queda "debiendo" hasta que se emite el
-- complemento que ampara el dinero. Sin este saldo no hay manera de saber
-- cuánto falta por pagar ni de armar el documento relacionado.
ALTER TABLE facturas ADD COLUMN metodo_pago TEXT NOT NULL DEFAULT 'PUE';
ALTER TABLE facturas ADD COLUMN saldo_cents INTEGER NOT NULL DEFAULT 0;

-- Cuánto de cada factura cubrió cada complemento. De aquí sale el
-- NumParcialidad (1 = primer pago de esa factura, 2 = el segundo...).
CREATE TABLE IF NOT EXISTS pagos_aplicados (
    pago_id     INTEGER NOT NULL REFERENCES facturas(id) ON DELETE CASCADE,
    factura_id  INTEGER NOT NULL REFERENCES facturas(id) ON DELETE CASCADE,
    monto_cents INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pagos_aplicados_factura ON pagos_aplicados(factura_id);
