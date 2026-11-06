-- Cancelación CFDI 4.0: motivo SAT, UUID que sustituye y el XML
-- que se deja en la carpeta para el contador.
--
-- Una factura cancelada deja de bloquear el periodo: se puede volver a
-- emitir la global de ese día.

ALTER TABLE facturas ADD COLUMN motivo_cancelacion TEXT NOT NULL DEFAULT '';
ALTER TABLE facturas ADD COLUMN uuid_sustitucion TEXT NOT NULL DEFAULT '';
ALTER TABLE facturas ADD COLUMN xml_cancelacion TEXT NOT NULL DEFAULT '';
ALTER TABLE facturas ADD COLUMN cancelada_at TEXT;

DROP INDEX IF EXISTS idx_facturas_periodo;
CREATE UNIQUE INDEX IF NOT EXISTS idx_facturas_periodo
    ON facturas(tipo, periodicidad, periodo)
    WHERE estado NOT IN ('error', 'cancelada') AND tipo <> 'egreso';
