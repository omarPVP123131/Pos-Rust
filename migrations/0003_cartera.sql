-- Cartera (fiados): cada abono queda registrado y auditado.
CREATE TABLE customer_payments (
    id          INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customers(id),
    amount_cents INTEGER NOT NULL,
    method      TEXT    NOT NULL DEFAULT 'efectivo',
    user_id     INTEGER,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_customer_payments ON customer_payments(customer_id, created_at);
