-- Conteo físico asistido.
--
-- El sistema dice cuánto debería haber; la persona cuenta lo que hay de verdad.
-- La diferencia es la merma (o el sobrante), y es el dato que nadie tiene y
-- todos quisieran: ahí se van las ganancias de una tienda.
--
-- Diseño: el conteo se guarda porque contar un anaquel toma horas y la tienda
-- sigue vendiendo mientras tanto. Si se cerrara la computadora a media cuenta,
-- no se pierde lo contado.

CREATE TABLE IF NOT EXISTS stock_counts (
    id           INTEGER PRIMARY KEY,
    creado_por   INTEGER, -- sin llave foránea: la bitácora tampoco la tiene
    alcance      TEXT    NOT NULL DEFAULT 'todos', -- todos|categoria
    filtro       TEXT    NOT NULL DEFAULT '',      -- la categoría, si aplica
    estado       TEXT    NOT NULL DEFAULT 'abierto', -- abierto|cerrado|aplicado
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    cerrado_at   TEXT,
    aplicado_por INTEGER
);

CREATE TABLE IF NOT EXISTS stock_count_lines (
    id              INTEGER PRIMARY KEY,
    count_id        INTEGER NOT NULL REFERENCES stock_counts(id) ON DELETE CASCADE,
    product_id      INTEGER NOT NULL REFERENCES products(id),
    -- Lo que el sistema decía cuando empezó el conteo (informativo).
    esperado_millis INTEGER NOT NULL,
    -- Lo que contó la persona. NULL = todavía no se cuenta.
    contado_millis  INTEGER,
    contado_at      TEXT,
    UNIQUE (count_id, product_id)
);

CREATE INDEX IF NOT EXISTS idx_count_lines_count ON stock_count_lines (count_id);
CREATE INDEX IF NOT EXISTS idx_counts_estado ON stock_counts (estado);
