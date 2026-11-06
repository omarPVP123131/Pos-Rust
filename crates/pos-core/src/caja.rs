//! Sesión de caja, arqueo y corte.
//!
//! El corte de caja es el momento de la verdad del mostrador: cuánto dinero
//! *debería* haber en el cajón contra cuánto *hay*. Este módulo hace esa cuenta
//! y es la razón por la que cada retiro y cada gasto lleva motivo.

use crate::{CoreError, Money, Result};

/// Denominaciones de efectivo en México, en centavos y con etiqueta.
/// Incluye monedas de 5, 10, 20 y 50 centavos porque en el arqueo real
/// aparecen (y porque medio peso por ticket es dinero al final del mes).
pub const DENOMINACIONES_MXN: [(i64, &str); 14] = [
    (5, "$0.05"),
    (10, "$0.10"),
    (20, "$0.20"),
    (50, "$0.50"),
    (100, "$1"),
    (200, "$2"),
    (500, "$5"),
    (1_000, "$10"),
    (2_000, "$20"),
    (5_000, "$50"),
    (10_000, "$100"),
    (20_000, "$200"),
    (50_000, "$500"),
    (100_000, "$1000"),
];

/// Conteo físico de billetes y monedas, denominación por denominación.
#[derive(Clone, Debug, PartialEq)]
pub struct Arqueo {
    /// (valor en centavos, cantidad de piezas), siempre en el orden de
    /// `DENOMINACIONES_MXN`.
    pub piezas: Vec<(i64, i64)>,
}

impl Default for Arqueo {
    fn default() -> Self {
        Self::new()
    }
}

impl Arqueo {
    /// Arqueo vacío: todas las denominaciones en cero.
    pub fn new() -> Self {
        Arqueo {
            piezas: DENOMINACIONES_MXN.iter().map(|(v, _)| (*v, 0)).collect(),
        }
    }

    pub fn set_piezas(&mut self, denominacion: i64, cantidad: i64) {
        if let Some(entry) = self.piezas.iter_mut().find(|(v, _)| *v == denominacion) {
            entry.1 = cantidad.max(0);
        }
    }

    pub fn get(&self, denominacion: i64) -> i64 {
        self.piezas
            .iter()
            .find(|(v, _)| *v == denominacion)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    /// Suma del dinero contado.
    pub fn total(&self) -> Money {
        self.piezas
            .iter()
            .fold(Money::ZERO, |acc, (valor, cantidad)| {
                acc + Money::from_cents(valor * cantidad)
            })
    }

    pub fn piezas_totales(&self) -> i64 {
        self.piezas.iter().map(|(_, c)| c).sum()
    }

    pub fn vacio(&self) -> bool {
        self.piezas.iter().all(|(_, c)| *c == 0)
    }

    pub fn limpiar(&mut self) {
        for (_, c) in self.piezas.iter_mut() {
            *c = 0;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstadoCaja {
    Abierta,
    Cerrada,
}

impl EstadoCaja {
    pub fn label(self) -> &'static str {
        match self {
            EstadoCaja::Abierta => "abierta",
            EstadoCaja::Cerrada => "cerrada",
        }
    }
}

/// Los movimientos que explican el dinero de un turno.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovimientoCaja {
    /// Dinero que se mete al cajón ("entrada").
    Entrada,
    /// Dinero que se saca para ir al banco o a la caja fuerte.
    Retiro,
    /// Papelería, café, transporte: sale del cajón y se comprueba después.
    Gasto,
}

impl MovimientoCaja {
    pub fn label(self) -> &'static str {
        match self {
            MovimientoCaja::Entrada => "Entrada",
            MovimientoCaja::Retiro => "Retiro",
            MovimientoCaja::Gasto => "Gasto",
        }
    }

    /// Signo con el que afecta al efectivo esperado en el cajón.
    pub fn signo(self) -> i64 {
        match self {
            MovimientoCaja::Entrada => 1,
            MovimientoCaja::Retiro | MovimientoCaja::Gasto => -1,
        }
    }
}

/// Todo lo que compone el efectivo que *debería* estar en el cajón.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CorteCaja {
    /// Con lo que se abrió la caja (fondo de cambio).
    pub fondo_inicial: Money,
    /// Efectivo neto de las ventas del turno (ya descontado el cambio
    /// entregado a los clientes).
    pub ventas_efectivo: Money,
    /// Entradas manuales de dinero.
    pub entradas: Money,
    /// Retiros al banco / caja fuerte.
    pub retiros: Money,
    /// Gastos menores.
    pub gastos: Money,
}

impl CorteCaja {
    /// El efectivo que debe haber físicamente en el cajón.
    pub fn esperado(&self) -> Money {
        let neto =
            self.fondo_inicial + self.ventas_efectivo + self.entradas - self.retiros - self.gastos;
        neto.max(Money::ZERO)
    }

    /// Cierra la caja comparando lo esperado contra lo contado.
    pub fn cerrar(self, contado: Money) -> Cierre {
        let esperado = self.esperado();
        Cierre {
            esperado,
            contado,
            diferencia: contado - esperado,
        }
    }

    pub fn total_vendido(&self) -> Money {
        self.ventas_efectivo
    }
}

/// Resultado del corte. La diferencia es el dato que nadie quiere ver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cierre {
    pub esperado: Money,
    pub contado: Money,
    /// Positivo = sobrante. Negativo = faltante.
    pub diferencia: Money,
}

impl Cierre {
    pub fn cuadra(&self) -> bool {
        self.diferencia.cents() == 0
    }

    pub fn sobrante(&self) -> Money {
        self.diferencia.max(Money::ZERO)
    }

    pub fn faltante(&self) -> Money {
        (Money::ZERO - self.diferencia).max(Money::ZERO)
    }
}

/// Valida que un retiro o gasto tenga sentido antes de tocar la base.
pub fn validar_movimiento(tipo: MovimientoCaja, monto: Money, motivo: &str) -> Result<()> {
    if monto.cents() <= 0 {
        return Err(CoreError::MontoInvalido(
            "El monto debe ser mayor a cero".into(),
        ));
    }
    if tipo != MovimientoCaja::Entrada && motivo.trim().is_empty() {
        return Err(CoreError::MontoInvalido(
            "Un retiro o gasto necesita motivo".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arqueo_ejemplo() -> Arqueo {
        let mut a = Arqueo::new();
        a.set_piezas(50, 4); // 4 monedas de 50 centavos = $2.00
        a.set_piezas(1_000, 3); // 3 billetes de $10 = $30.00
        a.set_piezas(10_000, 2); // 2 billetes de $100 = $200.00
        a.set_piezas(20_000, 1); // 1 billete de $200 = $200.00
        a
    }

    #[test]
    fn el_arqueo_suma_las_denominaciones() {
        let a = arqueo_ejemplo();
        assert_eq!(a.total(), Money::from_cents(43_200)); // $432.00
        assert_eq!(a.piezas_totales(), 10);
    }

    #[test]
    fn un_arqueo_vacio_cuenta_cero() {
        let a = Arqueo::new();
        assert!(a.vacio());
        assert_eq!(a.total(), Money::ZERO);
    }

    #[test]
    fn las_denominaciones_cubren_todo_el_efectivo_mexicano() {
        // De 5 centavos a 1000 pesos, sin huecos raros.
        assert_eq!(DENOMINACIONES_MXN.len(), 14);
        assert_eq!(DENOMINACIONES_MXN[0].0, 5);
        assert_eq!(DENOMINACIONES_MXN[13].0, 100_000);
    }

    #[test]
    fn el_efectivo_esperado_cuadra_con_un_turno_real() {
        let corte = CorteCaja {
            fondo_inicial: Money::from_cents(50_000),    // fondo de $500
            ventas_efectivo: Money::from_cents(383_500), // vendió $3,835 en efectivo
            entradas: Money::ZERO,
            retiros: Money::from_cents(200_000), // se llevó $2,000 al banco
            gastos: Money::from_cents(15_000),   // pagó $150 de papelería
        };
        assert_eq!(corte.esperado(), Money::from_cents(218_500));
    }

    #[test]
    fn el_corte_detecta_faltante_y_sobrante() {
        let corte = CorteCaja {
            fondo_inicial: Money::from_cents(50_000),
            ventas_efectivo: Money::from_cents(100_000),
            ..Default::default()
        };
        assert_eq!(corte.esperado(), Money::from_cents(150_000));

        // Contó de más: sobrante de $20
        let cierre = corte.cerrar(Money::from_cents(152_000));
        assert!(!cierre.cuadra());
        assert_eq!(cierre.diferencia, Money::from_cents(2_000));
        assert_eq!(cierre.sobrante(), Money::from_cents(2_000));
        assert_eq!(cierre.faltante(), Money::ZERO);

        // Contó de menos: faltante de $50
        let cierre = corte.cerrar(Money::from_cents(145_000));
        assert_eq!(cierre.diferencia, Money::from_cents(-5_000));
        assert_eq!(cierre.faltante(), Money::from_cents(5_000));
        assert_eq!(cierre.sobrante(), Money::ZERO);
    }

    #[test]
    fn un_corte_exacto_cuadra_al_centavo() {
        let corte = CorteCaja {
            fondo_inicial: Money::from_cents(50_000),
            ventas_efectivo: Money::from_cents(100_000),
            ..Default::default()
        };
        let cierre = corte.cerrar(Money::from_cents(150_000));
        assert!(cierre.cuadra());
    }

    #[test]
    fn el_efectivo_nunca_espera_negativo() {
        // Retiraron más de lo que había: esperado 0, no negativo.
        let corte = CorteCaja {
            fondo_inicial: Money::from_cents(10_000),
            ventas_efectivo: Money::ZERO,
            retiros: Money::from_cents(50_000),
            ..Default::default()
        };
        assert_eq!(corte.esperado(), Money::ZERO);
    }

    #[test]
    fn los_retiros_y_gastos_exigen_motivo() {
        assert!(validar_movimiento(MovimientoCaja::Retiro, Money::from_cents(100), "").is_err());
        assert!(
            validar_movimiento(MovimientoCaja::Retiro, Money::from_cents(100), "Al banco").is_ok()
        );
        assert!(validar_movimiento(MovimientoCaja::Entrada, Money::from_cents(100), "").is_ok());
        assert!(validar_movimiento(MovimientoCaja::Gasto, Money::ZERO, "Café").is_err());
    }

    #[test]
    fn el_signo_del_movimiento_afecta_bien_el_efectivo() {
        assert_eq!(MovimientoCaja::Entrada.signo(), 1);
        assert_eq!(MovimientoCaja::Retiro.signo(), -1);
        assert_eq!(MovimientoCaja::Gasto.signo(), -1);
    }
}
