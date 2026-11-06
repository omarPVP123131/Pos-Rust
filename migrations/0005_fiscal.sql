-- Facturación electrónica (CFDI 4.0).
--
-- Diseño: el comprobante se genera SIEMPRE y se guarda, aunque no se pueda
-- timbrar en ese momento (sin internet, sin PAC configurado, con el PAC caído).
-- Si el negocio depende de que el timbre salga en ese instante, se pierden
-- facturas; aquí la factura queda en la cola y se reintenta después.
--
-- El XML sin timbrar también se guarda: es el respaldo de lo que se vendió y
-- es lo que el contador necesita si el PAC se cae una semana.

CREATE TABLE IF NOT EXISTS facturas (
    id             INTEGER PRIMARY KEY,
    folio          TEXT    NOT NULL,
    tipo           TEXT    NOT NULL DEFAULT 'global', -- individual|global
    periodo        TEXT    NOT NULL DEFAULT '',       -- '2026-10-11' o '2026-10'
    periodicidad   TEXT    NOT NULL DEFAULT '01',     -- c_Periodicidad
    receptor_rfc   TEXT    NOT NULL DEFAULT '',
    receptor_nombre TEXT   NOT NULL DEFAULT '',
    subtotal_cents INTEGER NOT NULL DEFAULT 0,
    iva_cents      INTEGER NOT NULL DEFAULT 0,
    total_cents    INTEGER NOT NULL DEFAULT 0,
    ventas         TEXT    NOT NULL DEFAULT '', -- folios incluidos, separados por coma
    estado         TEXT    NOT NULL DEFAULT 'pendiente', -- pendiente|timbrada|error
    uuid           TEXT,
    xml            TEXT    NOT NULL DEFAULT '',  -- el comprobante generado
    xml_timbrado   TEXT,                         -- lo que devolvió el PAC
    intentos       INTEGER NOT NULL DEFAULT 0,
    ultimo_error   TEXT,
    creado_por     INTEGER,
    created_at     TEXT    NOT NULL DEFAULT (datetime('now')),
    timbrada_at    TEXT
);

-- Una sola factura global por periodo: si alguien le pica dos veces al botón
-- no salen dos comprobantes por las mismas ventas.
CREATE UNIQUE INDEX IF NOT EXISTS idx_facturas_periodo
    ON facturas(tipo, periodicidad, periodo) WHERE estado <> 'error';
CREATE INDEX IF NOT EXISTS idx_facturas_estado ON facturas(estado, created_at);
