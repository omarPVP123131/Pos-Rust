-- Pedidos a proveedor: se pide, llega, sube el inventario.
--
-- El stock no se edita a mano aquí: al recibir se escribe un movimiento
-- `compra` ligado al pedido. Si se corta la luz a medias, o todo el pedido
-- quedó o no se tocó nada (una sola transacción al recibir).

CREATE TABLE IF NOT EXISTS proveedores (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    phone      TEXT    NOT NULL DEFAULT '',
    nota       TEXT    NOT NULL DEFAULT '',
    is_active  INTEGER NOT NULL DEFAULT 1,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_proveedores_nombre ON proveedores(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS pedidos (
    id            INTEGER PRIMARY KEY,
    folio         TEXT    NOT NULL UNIQUE,
    proveedor_id  INTEGER NOT NULL REFERENCES proveedores(id),
    estado        TEXT    NOT NULL DEFAULT 'abierto', -- abierto|recibido|cancelado
    nota          TEXT    NOT NULL DEFAULT '',
    creado_por    INTEGER,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    recibido_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_pedidos_estado ON pedidos(estado, created_at);

CREATE TABLE IF NOT EXISTS pedido_lineas (
    id                   INTEGER PRIMARY KEY,
    pedido_id            INTEGER NOT NULL REFERENCES pedidos(id) ON DELETE CASCADE,
    product_id           INTEGER NOT NULL REFERENCES products(id),
    qty_pedida_millis    INTEGER NOT NULL,
    qty_recibida_millis  INTEGER NOT NULL DEFAULT 0,
    costo_cents          INTEGER NOT NULL DEFAULT 0,
    UNIQUE (pedido_id, product_id)
);

INSERT OR IGNORE INTO counters (name, value) VALUES ('pedido', 0);
