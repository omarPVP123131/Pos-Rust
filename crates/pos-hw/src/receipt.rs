//! Arma el ticket de venta a partir de una venta ya cerrada.
//!
//! `pos-hw` no conoce la base de datos: recibe una estructura plana. Así el
//! mismo código sirve para imprimir desde la venta en curso (UI) o para
//! reimprimir un ticket histórico (base de datos).

use pos_core::Money;

use crate::escpos::{Align, TicketBuilder};
use crate::perfil::Perfil;

#[derive(Clone, Debug)]
pub struct ReceiptItem {
    pub qty: String,
    pub description: String,
    pub importe: Money,
}

#[derive(Clone, Debug)]
pub struct ReceiptData {
    pub store_name: String,
    pub address: String,
    pub footer: String,
    pub folio: String,
    pub fecha: String,
    pub cajero: String,
    pub items: Vec<ReceiptItem>,
    pub subtotal: Money,
    pub descuento: Money,
    pub impuesto: Money,
    pub total: Money,
    pub pagos: Vec<(String, Money)>,
    pub cambio: Money,
    pub qr: Option<String>,
}

impl ReceiptData {
    /// Construye los bytes ESC/POS listos para mandar a la impresora.
    /// `abrir_cajon` se activa cuando hubo pago en efectivo.
    pub fn to_escpos(&self, width: usize, abrir_cajon: bool) -> Vec<u8> {
        self.to_escpos_con(&Perfil::generica_80().con_columnas(width), abrir_cajon)
    }

    /// Igual que [`Self::to_escpos`] pero respetando el perfil de la impresora
    /// (página de códigos, corte y cajón). Es el que usa la aplicación.
    pub fn to_escpos_con(&self, perfil: &Perfil, abrir_cajon: bool) -> Vec<u8> {
        let width = perfil.columnas;
        let mut t = TicketBuilder::con_perfil(perfil.clone());

        t.align(Align::Center)
            .bold(true)
            .line(&self.store_name)
            .bold(false);
        if !self.address.trim().is_empty() {
            t.line(&self.address);
        }
        t.separator();
        t.align(Align::Left)
            .kv("Folio", &self.folio)
            .kv("Fecha", &self.fecha)
            .kv("Cajero", &self.cajero)
            .separator();

        for item in &self.items {
            let encabezado = format!("{} x {}", item.qty, truncar(&item.description, width - 14));
            t.line(&encabezado);
            t.align(Align::Right).kv("", &item.importe.format_mxn());
            t.align(Align::Left);
        }

        t.separator().align(Align::Right);
        t.kv("Subtotal", &self.subtotal.format_mxn());
        if self.descuento.cents() > 0 {
            t.kv("Descuento", &format!("-{}", self.descuento.format_mxn()));
        }
        t.kv("IVA", &self.impuesto.format_mxn());
        t.bold(true).double(true);
        t.kv("TOTAL", &self.total.format_mxn());
        t.double(false).bold(false);

        t.separator();
        for (metodo, monto) in &self.pagos {
            t.kv(metodo, &monto.format_mxn());
        }
        if self.cambio.cents() > 0 {
            t.kv("CAMBIO", &self.cambio.format_mxn());
        }

        if let Some(qr) = &self.qr {
            t.feed(1).align(Align::Center).qr(qr);
        }

        t.feed(2).align(Align::Center).line(&self.footer).cortar();

        if abrir_cajon {
            t.open_drawer();
        }
        t.build()
    }
}

fn truncar(texto: &str, max: usize) -> String {
    if texto.chars().count() <= max {
        return texto.to_string();
    }
    texto
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>()
        + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datos() -> ReceiptData {
        ReceiptData {
            store_name: "Abarrotes Lupita".into(),
            address: "Av. Hidalgo 123, Xico".into(),
            footer: "¡Gracias por su compra!".into(),
            folio: "000123".into(),
            fecha: "2026-10-10 18:42".into(),
            cajero: "Ana".into(),
            items: vec![
                ReceiptItem {
                    qty: "2".into(),
                    description: "Coca-Cola 600 ml".into(),
                    importe: Money::from_cents(3600),
                },
                ReceiptItem {
                    qty: "1".into(),
                    description: "Pan blanco grande".into(),
                    importe: Money::from_cents(1850),
                },
            ],
            subtotal: Money::from_cents(4700),
            descuento: Money::ZERO,
            impuesto: Money::from_cents(750),
            total: Money::from_cents(5450),
            pagos: vec![("Efectivo".to_string(), Money::from_cents(5450))],
            cambio: Money::ZERO,
            qr: Some("https://mitienda.mx/t/000123".into()),
        }
    }

    #[test]
    fn el_ticket_incluye_todo_lo_que_pide_un_cliente() {
        let bytes = datos().to_escpos(48, true);
        let texto = String::from_utf8_lossy(&bytes);
        for esperado in [
            "Abarrotes Lupita",
            "000123",
            "Coca-Cola 600 ml",
            "$36.00",
            "TOTAL",
            "$54.50",
            "Efectivo",
            "Gracias",
        ] {
            assert!(texto.contains(esperado), "falta {esperado} en el ticket");
        }
        // Se mandó el pulso al cajón.
        assert!(bytes.windows(5).any(|w| w == [0x1B, b'p', 0, 25, 250]));
    }

    #[test]
    fn sin_efectivo_no_abre_el_cajon() {
        let mut d = datos();
        d.pagos = vec![("Tarjeta".into(), Money::from_cents(5450))];
        let bytes = d.to_escpos(48, false);
        assert!(!bytes.windows(5).any(|w| w == [0x1B, b'p', 0, 25, 250]));
    }

    #[test]
    fn el_ticket_cabe_en_58_mm() {
        // 32 columnas: ninguna línea debe dispararse de ancho.
        let bytes = datos().to_escpos(32, false);
        // Solo cuenta lo que realmente se imprime: fuera los comandos ESC/POS.
        let texto = String::from_utf8_lossy(&crate::escpos::strip_commands(&bytes)).to_string();
        for linea in texto.lines() {
            assert!(linea.chars().count() <= 32, "línea larga: {linea}");
        }
    }
}
