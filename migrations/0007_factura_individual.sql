-- Factura individual: cuando el cliente te dio su RFC, esa venta se factura
-- aparte y NO se mete en la global del día.

-- Los datos fiscales del receptor que el SAT pide y que hasta hoy no se
-- guardaban. Sin régimen, código postal y uso CFDI, una factura a nombre de
-- alguien no pasa.
ALTER TABLE customers ADD COLUMN regimen_fiscal TEXT NOT NULL DEFAULT '';
ALTER TABLE customers ADD COLUMN codigo_postal TEXT NOT NULL DEFAULT '';
ALTER TABLE customers ADD COLUMN uso_cfdi      TEXT NOT NULL DEFAULT 'S01';

-- Los mismos datos van también en la factura, porque el cliente puede
-- mudarse o cambiar de régimen el mes que viene, y una factura ya emitida
-- no puede cambiar junto con él.
ALTER TABLE facturas ADD COLUMN receptor_cp      TEXT NOT NULL DEFAULT '';
ALTER TABLE facturas ADD COLUMN receptor_regimen TEXT NOT NULL DEFAULT '';
ALTER TABLE facturas ADD COLUMN uso_cfdi         TEXT NOT NULL DEFAULT 'S01';
