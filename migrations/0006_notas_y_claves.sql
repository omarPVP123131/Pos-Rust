-- Devoluciones parciales y claves del SAT por producto.

-- Lo que ya se devolvió de cada venta, nota por nota.
--
-- Sin esta tabla no hay manera de saber si ya se devolvieron dos de las cinco
-- cocas, y una segunda nota volvería a devolver las mismas dos. Es el control
-- que permite que una venta tenga varias notas parciales sin pasarse.
CREATE TABLE IF NOT EXISTS notas_lineas (
    factura_id      INTEGER NOT NULL REFERENCES facturas(id) ON DELETE CASCADE,
    sku             TEXT    NOT NULL,
    cantidad_millis INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_notas_lineas_factura ON notas_lineas(factura_id);

-- Claves del catálogo del SAT, por producto.
--
-- Arrancan con las genéricas ('01010101' = "no existe en el catálogo", H87 =
-- pieza), que el SAT acepta. Tener la clave correcta evita observaciones del
-- contador, pero no es obligatorio para timbrar.
ALTER TABLE products ADD COLUMN clave_sat     TEXT NOT NULL DEFAULT '01010101';
ALTER TABLE products ADD COLUMN clave_unidad  TEXT NOT NULL DEFAULT 'H87';
ALTER TABLE products ADD COLUMN unidad_sat    TEXT NOT NULL DEFAULT 'Pieza';

-- Una venta puede tener VARIAS notas parciales (hoy devuelves dos cosas y
-- mañana tres), así que la llave única de "una nota por periodo" ya no aplica
-- a los egresos. El control de no pasarse lo lleva `notas_lineas`.
DROP INDEX IF EXISTS idx_facturas_periodo;
CREATE UNIQUE INDEX IF NOT EXISTS idx_facturas_periodo
    ON facturas(tipo, periodicidad, periodo)
    WHERE estado <> 'error' AND tipo <> 'egreso';
