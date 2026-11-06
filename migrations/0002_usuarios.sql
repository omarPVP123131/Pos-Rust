-- Control de acceso y bloqueo por intentos fallidos.
ALTER TABLE users ADD COLUMN failed_attempts INTEGER NOT NULL DEFAULT 0;

-- Bitácora de quién abrió y cerró la caja (para auditoría del corte).
ALTER TABLE cash_sessions ADD COLUMN closed_by INTEGER;
