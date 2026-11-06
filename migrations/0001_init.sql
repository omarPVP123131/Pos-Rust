-- Esquema inicial del punto de venta.
-- Convenciones:
--   * todo el dinero se guarda en centavos enteros (*_cents)
--   * las cantidades se guardan en milésimos (*_millis)
--   * las tasas se guardan en basis points (1600 = 16.00 %)
--   * fiscal_status y outbox nacen vacíos: la facturación CFDI (Fase 5)
--     se conecta después sin tocar este esquema.

CREATE TABLE tax_rates (
    id                  INTEGER PRIMARY KEY,
    name                TEXT    NOT NULL,
    percent_bp          INTEGER NOT NULL,          -- 1600 = 16.00 %
    is_included_in_price INTEGER NOT NULL DEFAULT 1,
    is_active           INTEGER NOT NULL DEFAULT 1
);
INSERT INTO tax_rates (id, name, percent_bp) VALUES
    (1, 'IVA 16%', 1600),
    (2, 'IVA 8%',   800),
    (3, 'IVA 0%',     0),
    (4, 'Exento',    -1);

CREATE TABLE categories (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE,
    parent_id INTEGER REFERENCES categories(id)
);

CREATE TABLE products (
    id               INTEGER PRIMARY KEY,
    sku              TEXT    NOT NULL UNIQUE,
    name             TEXT    NOT NULL,
    description      TEXT    NOT NULL DEFAULT '',
    category_id      INTEGER REFERENCES categories(id),
    unit             TEXT    NOT NULL DEFAULT 'pza',
    cost_cents       INTEGER NOT NULL DEFAULT 0,
    price_cents      INTEGER NOT NULL DEFAULT 0,
    tax_rate_id      INTEGER NOT NULL DEFAULT 1 REFERENCES tax_rates(id),
    track_inventory  INTEGER NOT NULL DEFAULT 1,
    stock_millis     INTEGER NOT NULL DEFAULT 0,
    min_stock_millis INTEGER NOT NULL DEFAULT 0,
    is_active        INTEGER NOT NULL DEFAULT 1,
    created_at       TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_products_name   ON products(name COLLATE NOCASE);
CREATE INDEX idx_products_active ON products(is_active, name);

-- Un producto puede tener varios códigos (presentación, proveedor, granel).
CREATE TABLE barcodes (
    product_id INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    code       TEXT    NOT NULL UNIQUE
);
CREATE INDEX idx_barcodes_code ON barcodes(code);

-- Libro mayor de existencias: el stock es la suma de estos movimientos.
CREATE TABLE inventory_movements (
    id               INTEGER PRIMARY KEY,
    product_id       INTEGER NOT NULL REFERENCES products(id),
    qty_delta_millis INTEGER NOT NULL,
    reason           TEXT    NOT NULL,   -- compra|venta|devolucion|ajuste|merma|conteo
    unit_cost_cents  INTEGER NOT NULL DEFAULT 0,
    ref_type         TEXT,
    ref_id           INTEGER,
    user_id          INTEGER,
    created_at       TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_mov_product ON inventory_movements(product_id, created_at);

CREATE TABLE cash_sessions (
    id                  INTEGER PRIMARY KEY,
    user_id             INTEGER,
    opened_at           TEXT    NOT NULL DEFAULT (datetime('now')),
    closed_at           TEXT,
    opening_float_cents INTEGER NOT NULL DEFAULT 0,
    expected_cents      INTEGER,
    counted_cents       INTEGER,
    difference_cents    INTEGER,
    status              TEXT    NOT NULL DEFAULT 'abierta'   -- abierta|cerrada
);

CREATE TABLE sales (
    id             INTEGER PRIMARY KEY,
    folio          TEXT    NOT NULL UNIQUE,
    cash_session_id INTEGER REFERENCES cash_sessions(id),
    user_id        INTEGER,
    customer_id    INTEGER,
    subtotal_cents INTEGER NOT NULL,
    discount_cents INTEGER NOT NULL DEFAULT 0,
    tax_cents      INTEGER NOT NULL,
    total_cents    INTEGER NOT NULL,
    status         TEXT    NOT NULL DEFAULT 'completada',  -- completada|cancelada|devuelta
    fiscal_status  TEXT    NOT NULL DEFAULT 'no_requerido', -- no_requerido|pendiente|timbrada
    note           TEXT    NOT NULL DEFAULT '',
    created_at     TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_sales_created ON sales(created_at);
CREATE INDEX idx_sales_status  ON sales(status);

CREATE TABLE sale_items (
    id                 INTEGER PRIMARY KEY,
    sale_id            INTEGER NOT NULL REFERENCES sales(id) ON DELETE CASCADE,
    product_id         INTEGER,
    sku                TEXT    NOT NULL,
    description        TEXT    NOT NULL,
    qty_millis         INTEGER NOT NULL,
    unit_price_cents   INTEGER NOT NULL,
    discount_cents     INTEGER NOT NULL DEFAULT 0,
    tax_bp             INTEGER NOT NULL DEFAULT 0,
    tax_cents          INTEGER NOT NULL DEFAULT 0,
    cost_cents_at_sale INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_items_sale ON sale_items(sale_id);

CREATE TABLE payments (
    id             INTEGER PRIMARY KEY,
    sale_id        INTEGER NOT NULL REFERENCES sales(id) ON DELETE CASCADE,
    method         TEXT    NOT NULL,   -- efectivo|tarjeta|transferencia|vales|credito
    amount_cents   INTEGER NOT NULL,
    received_cents INTEGER NOT NULL DEFAULT 0,
    reference      TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX idx_payments_sale ON payments(sale_id);

CREATE TABLE cash_movements (
    id           INTEGER PRIMARY KEY,
    session_id   INTEGER NOT NULL REFERENCES cash_sessions(id),
    type         TEXT    NOT NULL,   -- retiro|entrada|gasto
    amount_cents INTEGER NOT NULL,
    reason       TEXT    NOT NULL DEFAULT '',
    user_id      INTEGER,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE users (
    id           INTEGER PRIMARY KEY,
    username     TEXT    NOT NULL UNIQUE,
    display_name TEXT    NOT NULL,
    pin_hash     TEXT    NOT NULL,
    role         TEXT    NOT NULL DEFAULT 'cajero',  -- cajero|supervisor|admin
    is_active    INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE customers (
    id                 INTEGER PRIMARY KEY,
    name               TEXT NOT NULL,
    rfc                TEXT,
    email              TEXT,
    phone              TEXT,
    tax_regime         TEXT,
    cfdi_use           TEXT,
    credit_limit_cents INTEGER NOT NULL DEFAULT 0,
    balance_cents      INTEGER NOT NULL DEFAULT 0,
    is_active          INTEGER NOT NULL DEFAULT 1,
    created_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Solo se inserta; nunca se edita ni se borra.
CREATE TABLE audit_log (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER,
    action       TEXT NOT NULL,
    entity       TEXT NOT NULL,
    entity_id    INTEGER,
    payload_json TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Cola de sincronización. Hoy vacía; en la Fase 6 alimenta el servidor LAN
-- y en la Fase 5 alimenta al PAC para timbrar.
CREATE TABLE outbox (
    id           INTEGER PRIMARY KEY,
    entity       TEXT NOT NULL,
    entity_id    INTEGER NOT NULL,
    op           TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    sent_at      TEXT
);
CREATE INDEX idx_outbox_pending ON outbox(sent_at, created_at);

CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE counters (
    name  TEXT PRIMARY KEY,
    value INTEGER NOT NULL DEFAULT 0
);
INSERT INTO counters (name, value) VALUES ('folio', 0), ('cash_session', 0);

INSERT INTO settings (key, value) VALUES
    ('store_name',   'Mi Tienda'),
    ('store_address','Domicilio de la sucursal'),
    ('ticket_footer','¡Gracias por su compra!'),
    ('ticket_width', '48'),
    ('printer_path', ''),
    ('iva_bp',       '1600');
