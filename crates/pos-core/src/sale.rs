//! La venta: líneas, descuentos, totales y pagos.
//!
//! Modelo elegido: los precios de catálogo **incluyen impuestos** (como se
//! exhiben en el anaquel), y el IVA se separa al final por línea.

use crate::error::{CoreError, Result};
use crate::money::{div_round, Money};
use crate::tax::{tax_included_amount, TaxRate};
use crate::Quantity;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaleStatus {
    EnCaptura,
    Completada,
    Cancelada,
    Devuelta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentMethod {
    Efectivo,
    Tarjeta,
    Transferencia,
    Vales,
    Credito,
}

impl PaymentMethod {
    pub const ALL: [PaymentMethod; 5] = [
        PaymentMethod::Efectivo,
        PaymentMethod::Tarjeta,
        PaymentMethod::Transferencia,
        PaymentMethod::Vales,
        PaymentMethod::Credito,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PaymentMethod::Efectivo => "Efectivo",
            PaymentMethod::Tarjeta => "Tarjeta",
            PaymentMethod::Transferencia => "Transferencia",
            PaymentMethod::Vales => "Vales",
            PaymentMethod::Credito => "Crédito",
        }
    }

    /// Solo el efectivo entrega cambio y abre el cajón.
    pub fn gives_change(self) -> bool {
        matches!(self, PaymentMethod::Efectivo)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Payment {
    pub method: PaymentMethod,
    pub amount: Money,
    /// Cuánto entregó el cliente en ese método (efectivo).
    pub received: Money,
    pub reference: String,
}

impl Payment {
    pub fn new(method: PaymentMethod, amount: Money) -> Self {
        let received = if method.gives_change() {
            amount
        } else {
            Money::ZERO
        };
        Payment {
            method,
            amount,
            received,
            reference: String::new(),
        }
    }

    pub fn change(&self) -> Money {
        if self.received > self.amount {
            self.received - self.amount
        } else {
            Money::ZERO
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SaleLine {
    pub product_id: Option<i64>,
    pub sku: String,
    pub description: String,
    pub qty: Quantity,
    /// Precio unitario con impuesto incluido.
    pub unit_price: Money,
    /// Descuento aplicado al importe bruto de la línea.
    pub discount: Money,
    pub tax: TaxRate,
    /// Costo al momento de la venta: sin esto, el reporte de utilidad miente.
    pub unit_cost: Money,
}

impl SaleLine {
    /// Importe bruto de la línea (con impuesto, ya descontado).
    pub fn gross(&self) -> Money {
        let bruto = self.unit_price.mul_qty(self.qty);
        (bruto - self.discount.min(bruto)).max(Money::ZERO)
    }

    /// `(neto, impuesto)` de la línea.
    pub fn net_and_tax(&self) -> (Money, Money) {
        tax_included_amount(self.gross(), self.tax)
    }

    pub fn tax_amount(&self) -> Money {
        self.net_and_tax().1
    }

    /// Utilidad estimada de la línea (bruto con IVA - costo con IVA).
    pub fn profit(&self) -> Money {
        let costo = self.unit_cost.mul_qty(self.qty);
        (self.gross() - costo).max(Money::ZERO)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sale {
    pub id: Option<i64>,
    pub folio: String,
    pub lines: Vec<SaleLine>,
    /// Descuento global, repartido entre líneas al guardar.
    pub global_discount: Money,
    pub payments: Vec<Payment>,
    pub status: SaleStatus,
    pub customer_id: Option<i64>,
    pub note: String,
}

impl Sale {
    pub fn new(folio: impl Into<String>) -> Self {
        Sale {
            id: None,
            folio: folio.into(),
            lines: Vec::new(),
            global_discount: Money::ZERO,
            payments: Vec::new(),
            status: SaleStatus::EnCaptura,
            customer_id: None,
            note: String::new(),
        }
    }

    // ---------------------------------------------------------- líneas ----

    /// Agrega un producto; si ya está en el ticket, suma cantidad.
    pub fn add_line(&mut self, mut line: SaleLine) -> Result<()> {
        if line.qty.millis() <= 0 {
            return Err(CoreError::CantidadInvalida);
        }
        if line.unit_price.cents() < 0 {
            return Err(CoreError::PrecioInvalido);
        }
        if let Some(idx) = self
            .lines
            .iter()
            .position(|l| l.sku == line.sku && l.discount == line.discount)
        {
            let existente = &mut self.lines[idx];
            existente.qty = existente.qty + line.qty;
            // Dejamos constancia del costo más reciente.
            line.unit_cost = existente.unit_cost;
        } else {
            self.lines.push(line);
        }
        Ok(())
    }

    pub fn remove_line(&mut self, index: usize) {
        if index < self.lines.len() {
            self.lines.remove(index);
        }
    }

    pub fn change_qty(&mut self, index: usize, delta: Quantity) {
        if let Some(line) = self.lines.get_mut(index) {
            let nueva = line.qty + delta;
            if nueva.millis() > 0 {
                line.qty = nueva;
            }
        }
    }

    /// Pone la cantidad en seco: "quiero 12", no 12 veces +1.
    /// Cero o menos quita el renglón.
    pub fn set_qty(&mut self, index: usize, qty: Quantity) {
        if qty.millis() <= 0 {
            self.remove_line(index);
            return;
        }
        if let Some(line) = self.lines.get_mut(index) {
            line.qty = qty;
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.payments.clear();
        self.global_discount = Money::ZERO;
        self.customer_id = None;
        self.note.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    // --------------------------------------------------------- totales ----

    /// Suma de importes brutos por línea, sin descuento global.
    pub fn gross_subtotal(&self) -> Money {
        self.lines
            .iter()
            .fold(Money::ZERO, |acc, l| acc + l.gross())
    }

    /// Reparte el descuento global entre las líneas, proporcionalmente a su
    /// importe bruto. El último renglón absorbe el residuo del redondeo para
    /// que la suma sea exacta.
    fn allocated_global_discount(&self) -> Vec<Money> {
        let bruto = self.gross_subtotal();
        let descuento = self.effective_global_discount();
        let mut out = vec![Money::ZERO; self.lines.len()];
        if descuento.cents() == 0 || bruto.cents() == 0 {
            return out;
        }
        let mut acumulado = 0i64;
        let ultimo = self.lines.len().saturating_sub(1);
        for (i, linea) in self.lines.iter().enumerate() {
            let parte = if i == ultimo {
                descuento.cents() - acumulado
            } else {
                div_round(descuento.cents() * linea.gross().cents(), bruto.cents())
            };
            acumulado += parte;
            out[i] = Money::from_cents(parte);
        }
        out
    }

    /// Importe gravable de la línea `i` ya con su parte del descuento global.
    fn taxable(&self, index: usize, asignados: &[Money]) -> Money {
        (self.lines[index].gross() - asignados[index]).max(Money::ZERO)
    }

    /// `(neto, impuesto)` por línea, ya con el descuento global repartido.
    /// Es la única fuente de verdad para los totales y para lo que se guarda en
    /// la base de datos: así el ticket, la BD y el reporte siempre cuadran.
    pub fn tax_breakdown(&self) -> Vec<(Money, Money)> {
        let asignados = self.allocated_global_discount();
        (0..self.lines.len())
            .map(|i| tax_included_amount(self.taxable(i, &asignados), self.lines[i].tax))
            .collect()
    }

    /// Subtotal neto (sin impuestos), con el descuento global ya aplicado.
    pub fn subtotal(&self) -> Money {
        self.tax_breakdown()
            .iter()
            .fold(Money::ZERO, |acc, (neto, _)| acc + *neto)
    }

    pub fn tax_total(&self) -> Money {
        self.tax_breakdown()
            .iter()
            .fold(Money::ZERO, |acc, (_, iva)| acc + *iva)
    }

    /// Garantía contable: `total == subtotal + impuestos == bruto - descuento`.
    pub fn total(&self) -> Money {
        self.subtotal() + self.tax_total()
    }

    /// El descuento global nunca puede exceder el importe de la venta.
    pub fn effective_global_discount(&self) -> Money {
        self.global_discount.min(self.gross_subtotal())
    }

    pub fn discount_total(&self) -> Money {
        let por_linea = self
            .lines
            .iter()
            .fold(Money::ZERO, |acc, l| acc + l.discount);
        por_linea + self.effective_global_discount()
    }

    pub fn profit(&self) -> Money {
        self.lines
            .iter()
            .fold(Money::ZERO, |acc, l| acc + l.profit())
    }

    // ----------------------------------------------------------- pagos ----

    pub fn add_payment(&mut self, payment: Payment) {
        if payment.amount.cents() <= 0 {
            return;
        }
        self.payments.push(payment);
    }

    pub fn paid_total(&self) -> Money {
        self.payments
            .iter()
            .fold(Money::ZERO, |acc, p| acc + p.amount)
    }

    /// Lo que falta por cubrir (cero si ya está pagada o sobrepagada).
    pub fn balance_due(&self) -> Money {
        (self.total() - self.paid_total()).max(Money::ZERO)
    }

    /// Cambio a devolver (solo efectivo).
    pub fn change_due(&self) -> Money {
        self.payments
            .iter()
            .filter(|p| p.method.gives_change())
            .fold(Money::ZERO, |acc, p| acc + p.change())
    }

    pub fn is_fully_paid(&self) -> bool {
        !self.is_empty() && self.balance_due() == Money::ZERO
    }

    /// Cierra la venta. Falla si falta dinero o si no hay productos.
    pub fn complete(&mut self) -> Result<()> {
        if self.is_empty() {
            return Err(CoreError::VentaVacia);
        }
        if self.balance_due().cents() > 0 {
            return Err(CoreError::PagoInsuficiente {
                faltante: self.balance_due().cents(),
            });
        }
        self.status = SaleStatus::Completada;
        Ok(())
    }

    /// Aplica un descuento global validando que no reviente la venta.
    pub fn apply_global_discount(&mut self, descuento: Money) -> Result<()> {
        if descuento.cents() < 0 {
            return Err(CoreError::DescuentoExcesivo);
        }
        if descuento > self.gross_subtotal() {
            return Err(CoreError::DescuentoExcesivo);
        }
        self.global_discount = descuento;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linea(sku: &str, precio: i64, qty: i64, bp: i32) -> SaleLine {
        SaleLine {
            product_id: Some(1),
            sku: sku.to_string(),
            description: sku.to_string(),
            qty: Quantity::from_units(qty),
            unit_price: Money::from_cents(precio),
            discount: Money::ZERO,
            tax: TaxRate::from_bp(bp),
            unit_cost: Money::from_cents(precio / 2),
        }
    }

    #[test]
    fn una_venta_suma_lineas_y_separa_el_iva() {
        let mut venta = Sale::new("A-1");
        venta.add_line(linea("COCA600", 1800, 3, 1600)).unwrap();
        venta.add_line(linea("PAN-BCO", 1850, 1, 1600)).unwrap();

        // Bruto: 54.00 + 18.50 = 72.50
        assert_eq!(venta.gross_subtotal(), Money::from_cents(7250));
        // Neto: (5400 -> 4655) + (1850 -> 1595) = 6250
        assert_eq!(venta.subtotal(), Money::from_cents(6250));
        assert_eq!(venta.tax_total(), Money::from_cents(1000));
        // El total SIEMPRE es igual al bruto cuando no hay descuento.
        assert_eq!(venta.total(), Money::from_cents(7250));
    }

    #[test]
    fn poner_la_cantidad_en_seco_no_suma_de_a_uno() {
        let mut venta = Sale::new("A-qty");
        venta.add_line(linea("COCA", 1800, 1, 1600)).unwrap();
        venta.set_qty(0, Quantity::from_units(12));
        assert_eq!(venta.lines[0].qty, Quantity::from_units(12));
        venta.set_qty(0, Quantity::ZERO);
        assert!(venta.lines.is_empty());
    }

    #[test]
    fn productos_iguales_se_agrupan_en_una_sola_linea() {
        let mut venta = Sale::new("A-2");
        venta.add_line(linea("LECHE", 2600, 1, 1600)).unwrap();
        venta.add_line(linea("LECHE", 2600, 2, 1600)).unwrap();
        assert_eq!(venta.lines.len(), 1);
        assert_eq!(venta.lines[0].qty, Quantity::from_units(3));
    }

    #[test]
    fn el_total_cuadra_con_descuento_global() {
        let mut venta = Sale::new("A-3");
        venta.add_line(linea("LECHE", 2600, 3, 1600)).unwrap(); // 78.00
        venta
            .apply_global_discount(Money::from_cents(1160))
            .unwrap(); // -11.60
        assert_eq!(venta.total(), Money::from_cents(6640));
        assert_eq!(venta.subtotal(), Money::from_cents(5724));
        assert_eq!(venta.tax_total(), Money::from_cents(916));
    }

    #[test]
    fn no_se_puede_descontar_mas_que_la_venta() {
        let mut venta = Sale::new("A-4");
        venta.add_line(linea("PAN", 1850, 1, 1600)).unwrap();
        assert_eq!(
            venta.apply_global_discount(Money::from_cents(9999)),
            Err(CoreError::DescuentoExcesivo)
        );
    }

    #[test]
    fn pagos_mixtos_y_cambio_en_efectivo() {
        let mut venta = Sale::new("A-5");
        venta.add_line(linea("COCA", 1800, 3, 1600)).unwrap(); // 54.00

        // Paga 20 con tarjeta y 50 en efectivo: cambio 16.00
        venta.add_payment(Payment::new(
            PaymentMethod::Tarjeta,
            Money::from_cents(2000),
        ));
        let mut efectivo = Payment::new(PaymentMethod::Efectivo, Money::from_cents(3400));
        efectivo.received = Money::from_cents(5000);
        venta.add_payment(efectivo);

        assert_eq!(venta.paid_total(), Money::from_cents(5400));
        assert_eq!(venta.balance_due(), Money::ZERO);
        assert_eq!(venta.change_due(), Money::from_cents(1600));
        assert!(venta.is_fully_paid());
        venta.complete().unwrap();
        assert_eq!(venta.status, SaleStatus::Completada);
    }

    #[test]
    fn no_se_puede_cerrar_una_venta_sin_cubrir_el_total() {
        let mut venta = Sale::new("A-6");
        venta.add_line(linea("COCA", 1800, 1, 1600)).unwrap();
        venta.add_payment(Payment::new(
            PaymentMethod::Efectivo,
            Money::from_cents(1000),
        ));
        assert!(matches!(
            venta.complete(),
            Err(CoreError::PagoInsuficiente { .. })
        ));
    }

    #[test]
    fn una_venta_vacia_no_se_puede_cobrar() {
        let mut venta = Sale::new("A-7");
        assert_eq!(venta.complete(), Err(CoreError::VentaVacia));
    }

    #[test]
    fn la_utilidad_usael_costo_historico() {
        let mut venta = Sale::new("A-8");
        venta.add_line(linea("COCA", 1800, 2, 1600)).unwrap(); // costo 9.00 c/u
        assert_eq!(venta.profit(), Money::from_cents(1800)); // 36 - 18
    }
}
