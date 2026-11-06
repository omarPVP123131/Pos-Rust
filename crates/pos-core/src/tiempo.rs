//! Tiempo civil sin dependencias.
//!
//! El punto de venta se usa en México, así que la hora local importa: un
//! respaldo hecho a las 8 de la noche **debe** llamarse con la fecha de ese
//! día, no con la de mañana (que es lo que pasa si se usa UTC a secas).
//!
//! No hay `chrono` ni `time` a propósito: este crate no tiene dependencias y
//! se compila en milisegundos. La conversión es el algoritmo
//! *civil_from_days* de Howard Hinnant (dominio público), que es exacto para
//! cualquier fecha del rango `i64`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Desfase de la Ciudad de México (UTC−6, sin horario de verano desde 2022).
pub const OFFSET_CDMX_SEGS: i64 = -6 * 3600;

/// Segundos desde el epoch. Nunca falla: si el reloj está mal, da 0.
pub fn ahora_segs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Convierte un número de días desde el epoch a (año, mes, día).
///
/// Algoritmo *civil_from_days* de Howard Hinnant (dominio público).
pub fn civil_desde_dias(dias: i64) -> (i64, u32, u32) {
    let z = dias + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Una fecha y hora ya convertidas a hora local, listas para mostrarse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FechaHora {
    pub anio: i64,
    pub mes: u32,
    pub dia: u32,
    pub hora: u32,
    pub minuto: u32,
    pub segundo: u32,
}

impl FechaHora {
    /// Interpreta `segs` (epoch UTC) con un desfase dado en segundos.
    pub fn desde_segs(segs: i64, offset_segs: i64) -> FechaHora {
        let local = segs + offset_segs;
        let dias = local.div_euclid(86_400);
        let resto = local.rem_euclid(86_400);
        let (anio, mes, dia) = civil_desde_dias(dias);
        FechaHora {
            anio,
            mes,
            dia,
            hora: (resto / 3600) as u32,
            minuto: (resto % 3600 / 60) as u32,
            segundo: (resto % 60) as u32,
        }
    }

    /// Hora local de la Ciudad de México.
    pub fn cdmx(segs: i64) -> FechaHora {
        FechaHora::desde_segs(segs, OFFSET_CDMX_SEGS)
    }

    /// `2026-10-10`
    pub fn fecha(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.anio, self.mes, self.dia)
    }

    /// `18:15`
    pub fn hora_minuto(&self) -> String {
        format!("{:02}:{:02}", self.hora, self.minuto)
    }

    /// `2026-10-10 18:15:00`
    pub fn fecha_hora(&self) -> String {
        format!(
            "{} {:02}:{:02}:{:02}",
            self.fecha(),
            self.hora,
            self.minuto,
            self.segundo
        )
    }

    /// Para nombres de archivo: `20261010-181500`. Ordena bien alfabéticamente.
    pub fn compacto(&self) -> String {
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            self.anio, self.mes, self.dia, self.hora, self.minuto, self.segundo
        )
    }

    /// Días civiles desde el epoch: sirve para comparar "¿esto es de hoy?".
    ///
    /// Es `days_from_civil` de Howard Hinnant, la inversa exacta de
    /// [`civil_desde_dias`]. Se usa la versión canónica (con año basado en
    /// enero) porque una versión "a ojo" se equivoca por un día justo en los
    /// años bisiestos, o sea, cada cuatro años.
    pub fn dia_civil(&self) -> i64 {
        let y = if self.mes <= 2 {
            self.anio - 1
        } else {
            self.anio
        };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = if self.mes > 2 {
            self.mes - 3
        } else {
            self.mes + 9
        } as i64;
        let doy = (153 * mp + 2) / 5 + self.dia as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_algoritmo_civil_acierta_en_fechas_conocidas() {
        // 1970-01-01 es el día 0.
        assert_eq!(civil_desde_dias(0), (1970, 1, 1));
        // 2000-02-29: año bisiesto secular.
        assert_eq!(civil_desde_dias(11_016), (2000, 2, 29));
        // 2026-10-10: hoy.
        assert_eq!(civil_desde_dias(20_736), (2026, 10, 10));
        // Antes del epoch también funciona.
        assert_eq!(civil_desde_dias(-1), (1969, 12, 31));
    }

    #[test]
    fn la_hora_local_no_se_corre_de_dia() {
        // 2026-10-11 02:00 UTC = 2026-10-10 20:00 en la Ciudad de México.
        let segs = 20_737 * 86_400 + 2 * 3600;
        let cdmx = FechaHora::cdmx(segs);
        assert_eq!(cdmx.fecha(), "2026-10-10");
        assert_eq!(cdmx.hora_minuto(), "20:00");
        assert_eq!(cdmx.compacto(), "20261010-200000");
    }

    #[test]
    fn el_formato_compacto_ordena_bien() {
        let a = FechaHora::cdmx(20_700 * 86_400 + 9 * 3600);
        let b = FechaHora::cdmx(20_701 * 86_400 + 9 * 3600);
        assert!(a.compacto() < b.compacto());
        assert_eq!(a.compacto().len(), 15);
    }

    #[test]
    fn el_dia_civil_es_estable_y_concuerda_con_el_algoritmo_inverso() {
        for dias in [-1i64, 0, 1, 11_016, 20_736, 30_000] {
            let (a, m, d) = civil_desde_dias(dias);
            let fh = FechaHora {
                anio: a,
                mes: m,
                dia: d,
                hora: 0,
                minuto: 0,
                segundo: 0,
            };
            assert_eq!(fh.dia_civil(), dias, "ida y vuelta en {dias}");
        }
    }

    #[test]
    fn los_bisiestos_se_cuentan_bien() {
        assert_eq!(
            FechaHora::cdmx(11_016 * 86_400 + 6 * 3600).fecha(),
            "2000-02-29"
        );
        assert_eq!(
            FechaHora::cdmx(10_650 * 86_400 + 6 * 3600).fecha(),
            "1999-02-28"
        );
    }
}
