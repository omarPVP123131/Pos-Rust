//! Dinero y cantidades en punto fijo (enteros).
//!
//! Nunca se usa `f64` para dinero: todo son **centavos** (`i64`).
//! Las cantidades viven en **milésimos** (`1.750 kg` = `1750`), para soportar
//! productos pesables sin errores de redondeo.

use std::fmt;
use std::ops::{Add, Sub};

/// Escala del dinero: centavos por unidad (peso).
pub const MONEY_SCALE: i64 = 100;
/// Escala de las cantidades: milésimos por unidad.
pub const QTY_SCALE: i64 = 1000;

/// División con redondeo "half away from zero" (medio, hacia afuera).
/// Es el criterio que usa el SAT para el IVA.
pub fn div_round(num: i64, den: i64) -> i64 {
    assert!(den != 0, "división por cero");
    let negativo = (num < 0) ^ (den < 0);
    let n = num.unsigned_abs();
    let d = den.unsigned_abs();
    let q = (2 * n + d) / (2 * d);
    if negativo {
        -(q as i64)
    } else {
        q as i64
    }
}

// ---------------------------------------------------------------- Money ----

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(i64); // centavos

impl Money {
    pub const ZERO: Money = Money(0);

    #[inline]
    pub const fn from_cents(cents: i64) -> Self {
        Money(cents)
    }

    /// Solo para importar catálogos o capturar en la UI; pierde precisión
    /// por diseño. En cuanto entra al sistema se convierte a centavos.
    pub fn from_f64(amount: f64) -> Self {
        Money((amount * MONEY_SCALE as f64).round() as i64)
    }

    /// Convierte un monto escalado (por ejemplo milésimos) a centavos.
    pub fn from_scaled(scaled: i64, scale: i64) -> Self {
        Money(div_round(scaled * MONEY_SCALE, scale))
    }

    #[inline]
    pub const fn cents(self) -> i64 {
        self.0
    }

    pub fn as_f64(self) -> f64 {
        self.0 as f64 / MONEY_SCALE as f64
    }

    pub const fn checked_add(self, other: Money) -> Option<Money> {
        match self.0.checked_add(other.0) {
            Some(v) => Some(Money(v)),
            None => None,
        }
    }

    pub const fn checked_sub(self, other: Money) -> Option<Money> {
        match self.0.checked_sub(other.0) {
            Some(v) => Some(Money(v)),
            None => None,
        }
    }

    /// Precio unitario × cantidad, redondeando a centavos.
    pub fn mul_qty(self, qty: Quantity) -> Money {
        Money(div_round(self.0 * qty.millis(), QTY_SCALE))
    }

    /// Importe × porcentaje en basis points (1600 = 16.00%).
    pub fn apply_bp(self, basis_points: i32) -> Money {
        Money(div_round(self.0 * basis_points as i64, 10_000))
    }

    pub fn max(self, other: Money) -> Money {
        if self.0 >= other.0 {
            self
        } else {
            other
        }
    }

    pub fn min(self, other: Money) -> Money {
        if self.0 <= other.0 {
            self
        } else {
            other
        }
    }

    /// `$1,234.56`
    pub fn format_mxn(self) -> String {
        let signo = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        format!("{}${}.{:02}", signo, group_thousands(abs / 100), abs % 100)
    }

    /// Parsea captura humana: "18", "18.5", "18.50", "$1,234.56".
    pub fn parse(input: &str) -> Option<Money> {
        let limpio: String = input
            .trim()
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',' || *c == '-')
            .collect();
        if !limpio.chars().any(|c| c.is_ascii_digit()) {
            return None;
        }
        let limpio = limpio.replace(',', "");
        let negativo = limpio.starts_with('-');
        let limpio = limpio.trim_start_matches('-');
        let (enteros, centavos) = match limpio.split_once('.') {
            Some((e, d)) => (e, d),
            None => (limpio, ""),
        };
        let e: i64 = enteros.parse().unwrap_or(0);
        let d = format!("{:0<2}", centavos.chars().take(2).collect::<String>());
        let c: i64 = d.parse().unwrap_or(0);
        let total = e * MONEY_SCALE + c;
        Some(Money(if negativo { -total } else { total }))
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Money) -> Money {
        Money(self.0.checked_add(rhs.0).expect("desbordamiento de dinero"))
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Money) -> Money {
        Money(self.0.checked_sub(rhs.0).expect("desbordamiento de dinero"))
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_mxn())
    }
}

impl fmt::Debug for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Money({}c)", self.0)
    }
}

// ------------------------------------------------------------- Quantity ----

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Quantity(i64); // milésimos

impl Quantity {
    pub const ZERO: Quantity = Quantity(0);
    pub const ONE: Quantity = Quantity(QTY_SCALE);

    #[inline]
    pub const fn from_millis(millis: i64) -> Self {
        Quantity(millis)
    }

    #[inline]
    pub const fn from_units(units: i64) -> Self {
        Quantity(units * QTY_SCALE)
    }

    /// 1.5 -> 1500 milésimos
    pub fn from_f64(value: f64) -> Self {
        Quantity((value * QTY_SCALE as f64).round() as i64)
    }

    #[inline]
    pub const fn millis(self) -> i64 {
        self.0
    }

    pub fn as_f64(self) -> f64 {
        self.0 as f64 / QTY_SCALE as f64
    }

    /// "1.5", "3", "0.750"
    pub fn format(self) -> String {
        let signo = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let enteros = abs / QTY_SCALE as u64;
        let millis = abs % QTY_SCALE as u64;
        if millis == 0 {
            format!("{signo}{enteros}")
        } else {
            let dec = format!("{millis:03}");
            format!("{signo}{enteros}.{}", dec.trim_end_matches('0'))
        }
    }

    /// Parsea "1", "1.5", "1,250"
    pub fn parse(input: &str) -> Option<Quantity> {
        let limpio: String = input
            .trim()
            .replace(',', ".")
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        let negativo = limpio.starts_with('-');
        if !limpio.chars().any(|c| c.is_ascii_digit()) {
            return None;
        }
        let limpio = limpio.trim_start_matches('-');
        let (enteros, frac) = match limpio.split_once('.') {
            Some((e, d)) => (e, d),
            None => (limpio, ""),
        };
        let e: i64 = enteros.parse().unwrap_or(0);
        let d = format!("{:0<3}", frac.chars().take(3).collect::<String>());
        let m: i64 = d.parse().unwrap_or(0);
        let total = e * QTY_SCALE + m;
        Some(Quantity(if negativo { -total } else { total }))
    }
}

impl Add for Quantity {
    type Output = Quantity;
    fn add(self, rhs: Quantity) -> Quantity {
        Quantity(self.0 + rhs.0)
    }
}

impl Sub for Quantity {
    type Output = Quantity;
    fn sub(self, rhs: Quantity) -> Quantity {
        Quantity(self.0 - rhs.0)
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format())
    }
}

impl fmt::Debug for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Quantity({}m)", self.0)
    }
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dinero_se_formatea_en_mxn() {
        assert_eq!(Money::from_cents(0).format_mxn(), "$0.00");
        assert_eq!(Money::from_cents(5).format_mxn(), "$0.05");
        assert_eq!(Money::from_cents(15370).format_mxn(), "$153.70");
        assert_eq!(Money::from_cents(123456789).format_mxn(), "$1,234,567.89");
        assert_eq!(Money::from_cents(-15370).format_mxn(), "-$153.70");
    }

    #[test]
    fn parsea_montos_escritos_por_humanos() {
        assert_eq!(Money::parse("18"), Some(Money::from_cents(1800)));
        assert_eq!(Money::parse("18.5"), Some(Money::from_cents(1850)));
        assert_eq!(Money::parse("18.50"), Some(Money::from_cents(1850)));
        assert_eq!(Money::parse("$1,234.56"), Some(Money::from_cents(123456)));
        assert_eq!(Money::parse("abc"), None);
        assert_eq!(Money::parse(""), None);
    }

    #[test]
    fn redondeo_half_away_from_zero() {
        assert_eq!(div_round(5, 2), 3);
        assert_eq!(div_round(7, 2), 4);
        assert_eq!(div_round(-5, 2), -3);
        assert_eq!(div_round(1, 3), 0);
        assert_eq!(div_round(2, 3), 1);
    }

    #[test]
    fn precio_por_cantidad_redondea_a_centavos() {
        // $18.50 x 1.750 kg = $32.375 -> $32.38
        assert_eq!(
            Money::from_cents(1850).mul_qty(Quantity::from_millis(1750)),
            Money::from_cents(3238)
        );
        // 3 x $26.00 = $78.00 exacto
        assert_eq!(
            Money::from_cents(2600).mul_qty(Quantity::from_units(3)),
            Money::from_cents(7800)
        );
    }

    #[test]
    fn cantidades_se_formatean_sin_ceros_de_mas() {
        assert_eq!(Quantity::from_units(3).format(), "3");
        assert_eq!(Quantity::from_millis(1750).format(), "1.75");
        assert_eq!(Quantity::from_millis(100).format(), "0.1");
        assert_eq!(Quantity::parse("1.5"), Some(Quantity::from_millis(1500)));
        assert_eq!(Quantity::parse("2,250"), Some(Quantity::from_millis(2250)));
    }

    #[test]
    fn basis_points_aplican_porcentaje() {
        assert_eq!(
            Money::from_cents(10000).apply_bp(1600),
            Money::from_cents(1600)
        );
        assert_eq!(Money::from_cents(333).apply_bp(1600), Money::from_cents(53));
    }
}
