//! Impuestos: IVA, IEPS, exento.
//!
//! Las tasas NO se queman en el código: viven en la tabla `tax_rates` y viajan
//! como `basis_points` (1600 = 16.00 %) para no arrastrar decimales.
//!
//! En México el precio de mostrador **incluye** impuestos, así que la operación
//! crítica es la inversa: dado el importe bruto, separar el IVA.

use crate::money::{div_round, Money};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaxRate {
    pub basis_points: i32,
}

impl TaxRate {
    /// IVA 16 % — el caso común en retail.
    pub const IVA_16: TaxRate = TaxRate { basis_points: 1600 };
    /// IVA 8 % — región fronteriza.
    pub const IVA_8: TaxRate = TaxRate { basis_points: 800 };
    /// Tasa 0 % (se declara, pero no cobra impuesto).
    pub const IVA_0: TaxRate = TaxRate { basis_points: 0 };
    /// Exento (no se declara).
    pub const EXENTO: TaxRate = TaxRate { basis_points: -1 };

    pub const fn from_bp(bp: i32) -> Self {
        TaxRate { basis_points: bp }
    }

    pub fn is_exempt(self) -> bool {
        self.basis_points <= 0
    }

    pub fn percent_str(self) -> String {
        if self.basis_points < 0 {
            return "Exento".to_string();
        }
        format!("{}%", self.basis_points as f64 / 100.0)
    }
}

/// Separa el ISR... mejor dicho, el IVA de un importe que **ya lo incluye**.
///
/// Ejemplo: $116.00 con IVA 16 % -> neto $100.00, impuesto $16.00.
pub fn tax_included_amount(gross: Money, rate: TaxRate) -> (Money, Money) {
    if rate.is_exempt() {
        return (gross, Money::ZERO);
    }
    let net = Money::from_cents(div_round(
        gross.cents() * 10_000,
        10_000 + rate.basis_points as i64,
    ));
    (net, gross - net)
}

/// Calcula el impuesto que se **suma** a un importe neto (precio sin impuesto).
pub fn tax_added_amount(net: Money, rate: TaxRate) -> Money {
    if rate.is_exempt() {
        return Money::ZERO;
    }
    Money::from_cents(div_round(net.cents() * rate.basis_points as i64, 10_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separa_el_iva_de_un_precio_con_impuesto_incluido() {
        let (neto, iva) = tax_included_amount(Money::from_cents(11600), TaxRate::IVA_16);
        assert_eq!(neto, Money::from_cents(10000));
        assert_eq!(iva, Money::from_cents(1600));
        assert_eq!(neto + iva, Money::from_cents(11600));
    }

    #[test]
    fn el_iva_nunca_pierde_centavos() {
        // $18.00 con IVA incluido: neto 15.52, IVA 2.48 (suma exacta 18.00)
        let (neto, iva) = tax_included_amount(Money::from_cents(1800), TaxRate::IVA_16);
        assert_eq!(neto + iva, Money::from_cents(1800));
        assert_eq!(neto, Money::from_cents(1552));
        assert_eq!(iva, Money::from_cents(248));
    }

    #[test]
    fn tasa_cero_y_exento_no_cobran_impuesto() {
        let (neto, iva) = tax_included_amount(Money::from_cents(5000), TaxRate::IVA_0);
        assert_eq!((neto, iva), (Money::from_cents(5000), Money::ZERO));

        let (neto, iva) = tax_included_amount(Money::from_cents(5000), TaxRate::EXENTO);
        assert_eq!((neto, iva), (Money::from_cents(5000), Money::ZERO));
    }

    #[test]
    fn impuesto_sobre_neto_redondea_correctamente() {
        assert_eq!(
            tax_added_amount(Money::from_cents(10000), TaxRate::IVA_16),
            Money::from_cents(1600)
        );
        // 15.52 * 16% = 2.4832 -> 2.48
        assert_eq!(
            tax_added_amount(Money::from_cents(1552), TaxRate::IVA_16),
            Money::from_cents(248)
        );
    }
}
