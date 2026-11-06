//! Usuarios, roles y permisos del punto de venta.
//!
//! El hash del PIN **no** vive aquí: eso es responsabilidad de `pos-db`.
//! Este módulo decide *quién puede hacer qué*, que es la parte que de verdad
//! importa cuando hay diferencias de caja al final del día.

use crate::CoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Cobra. Nada más.
    Cajero,
    /// Puede descontar, devolver y retirar efectivo.
    Supervisor,
    /// Control total: usuarios, configuración y cierre de caja.
    Admin,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Cajero, Role::Supervisor, Role::Admin];

    pub fn label(self) -> &'static str {
        match self {
            Role::Cajero => "Cajero",
            Role::Supervisor => "Supervisor",
            Role::Admin => "Administrador",
        }
    }

    pub fn desde_texto(s: &str) -> Role {
        match s.trim().to_lowercase().as_str() {
            "admin" | "administrador" => Role::Admin,
            "supervisor" => Role::Supervisor,
            _ => Role::Cajero,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Cajero => "cajero",
            Role::Supervisor => "supervisor",
            Role::Admin => "admin",
        }
    }
}

/// Operaciones que requieren autorización. Cada una deja rastro en la bitácora.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permiso {
    Descuento,
    PrecioManual,
    Devolucion,
    CancelarVenta,
    RetiroEfectivo,
    OtorgarCredito,
    /// Firmar un corte que no cuadra (faltante o sobrante).
    AutorizarDiferencia,
    CerrarCaja,
    /// Ver la bitácora: quién hizo qué y quién autorizó qué.
    VerAuditoria,
    /// Ajustar el inventario (merma, merma por conteo, corrección a mano).
    AjusteInventario,
    Configuracion,
    /// Emitir comprobantes fiscales (factura global, notas de crédito).
    ///
    /// No es lo mismo que `Configuracion`: cambiar el RFC del negocio lo hace
    /// el dueño una vez, pero facturar el día lo tiene que poder hacer quien
    /// quede encargado. Por eso el supervisor sí puede y el cajero no.
    Facturar,
    Usuarios,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

impl std::fmt::Display for Permiso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.descripcion())
    }
}

impl Permiso {
    pub fn descripcion(self) -> &'static str {
        match self {
            Permiso::Descuento => "aplicar un descuento",
            Permiso::PrecioManual => "cambiar el precio a mano",
            Permiso::Devolucion => "hacer una devolución",
            Permiso::CancelarVenta => "cancelar una venta",
            Permiso::RetiroEfectivo => "retirar efectivo de la caja",
            Permiso::OtorgarCredito => "vender a crédito (fiado)",
            Permiso::AutorizarDiferencia => "cerrar una caja que no cuadra",
            Permiso::CerrarCaja => "cerrar la caja",
            Permiso::VerAuditoria => "ver la bitácora",
            Permiso::AjusteInventario => "ajustar el inventario",
            Permiso::Configuracion => "cambiar la configuración",
            Permiso::Facturar => "emitir facturas (CFDI)",
            Permiso::Usuarios => "administrar usuarios",
        }
    }
}

impl Role {
    pub fn puede(self, permiso: Permiso) -> bool {
        match self {
            Role::Admin => true,
            Role::Supervisor => matches!(
                permiso,
                Permiso::Descuento
                    | Permiso::PrecioManual
                    | Permiso::Devolucion
                    | Permiso::CancelarVenta
                    | Permiso::RetiroEfectivo
                    | Permiso::OtorgarCredito
                    | Permiso::AutorizarDiferencia
                    | Permiso::VerAuditoria
                    | Permiso::AjusteInventario
                    | Permiso::Facturar
                    | Permiso::CerrarCaja
            ),
            // Cobra, corta su caja y da de alta precio. No configura ni
            // mueve inventario a mano: eso es del encargado.
            Role::Cajero => matches!(permiso, Permiso::CerrarCaja),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub is_active: bool,
    pub failed_attempts: i32,
}

impl User {
    pub fn puede(&self, permiso: Permiso) -> bool {
        if !self.is_active {
            return false;
        }
        self.role.puede(permiso)
    }

    /// Si no puede, devuelve el error listo para mostrarse al cajero.
    pub fn exigir(&self, permiso: Permiso) -> Result<(), CoreError> {
        if self.puede(permiso) {
            Ok(())
        } else {
            Err(CoreError::RequiereSupervisor(permiso.descripcion()))
        }
    }

    pub fn bloqueado(&self, max_intentos: i32) -> bool {
        self.failed_attempts >= max_intentos
    }
}

/// Intentos permitidos antes de bloquear al usuario.
pub const MAX_INTENTOS_PIN: i32 = 5;
pub const PIN_MIN: usize = 4;
pub const PIN_MAX: usize = 8;

/// PIN de 4 a 8 dígitos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin(String);

impl Pin {
    pub fn parse(texto: &str) -> Result<Self, CoreError> {
        let limpio = texto.trim();
        if limpio.len() < PIN_MIN || limpio.len() > PIN_MAX {
            return Err(CoreError::PinInvalido);
        }
        if !limpio.chars().all(|c| c.is_ascii_digit()) {
            return Err(CoreError::PinInvalido);
        }
        Ok(Pin(limpio.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_cajero_no_puede_descontar_ni_retirar() {
        let cajero = User {
            id: 1,
            username: "ana".into(),
            display_name: "Ana".into(),
            role: Role::Cajero,
            is_active: true,
            failed_attempts: 0,
        };
        assert!(!cajero.puede(Permiso::Descuento));
        assert!(!cajero.puede(Permiso::RetiroEfectivo));
        assert!(cajero.puede(Permiso::CerrarCaja));
        assert!(!cajero.puede(Permiso::Configuracion));
        // La bitácora no es para cualquiera: ahí se ve quién autorizó qué.
        assert!(!cajero.puede(Permiso::VerAuditoria));
        assert!(cajero.exigir(Permiso::Descuento).is_err());
    }

    #[test]
    fn el_supervisor_desdega_pero_no_configura() {
        let sup = User {
            id: 2,
            username: "luis".into(),
            display_name: "Luis".into(),
            role: Role::Supervisor,
            is_active: true,
            failed_attempts: 0,
        };
        assert!(sup.puede(Permiso::Descuento));
        assert!(sup.puede(Permiso::Devolucion));
        assert!(!sup.puede(Permiso::Configuracion));
        assert!(sup.puede(Permiso::CerrarCaja));
        assert!(sup.puede(Permiso::OtorgarCredito));
        // Ver la bitácora sí: es justo lo que necesita para investigar.
        assert!(sup.puede(Permiso::VerAuditoria));
    }

    #[test]
    fn ajustar_inventario_lo_firma_un_encargado() {
        let admin = Role::Admin;
        let sup = Role::Supervisor;
        let cajero = Role::Cajero;
        assert!(admin.puede(Permiso::AjusteInventario));
        assert!(sup.puede(Permiso::AjusteInventario));
        // Un cajero no puede mover el inventario a mano: es la puerta
        // por donde se va el dinero.
        assert!(!cajero.puede(Permiso::AjusteInventario));
        assert_eq!(
            Permiso::AjusteInventario.descripcion(),
            "ajustar el inventario"
        );
        assert_eq!(
            Permiso::AjusteInventario.to_string(),
            "ajustar el inventario"
        );
    }

    #[test]
    fn el_admin_puede_todo_y_un_inactivo_nada() {
        let admin = User {
            id: 3,
            username: "root".into(),
            display_name: "Root".into(),
            role: Role::Admin,
            is_active: true,
            failed_attempts: 0,
        };
        for p in [
            Permiso::Descuento,
            Permiso::RetiroEfectivo,
            Permiso::CerrarCaja,
            Permiso::Usuarios,
        ] {
            assert!(admin.puede(p));
        }

        let mut inactivo = admin.clone();
        inactivo.is_active = false;
        assert!(!inactivo.puede(Permiso::Usuarios));
    }

    #[test]
    fn el_pin_acepta_entre_4_y_8_digitos() {
        assert!(Pin::parse("1234").is_ok());
        assert!(Pin::parse("12345678").is_ok());
        assert!(Pin::parse("123").is_err());
        assert!(Pin::parse("123456789").is_err());
        assert!(Pin::parse("12a4").is_err());
        assert_eq!(Pin::parse(" 4321 ").unwrap().as_str(), "4321");
    }

    #[test]
    fn se_bloquea_tras_varios_intentos() {
        let mut u = User {
            id: 4,
            username: "ana".into(),
            display_name: "Ana".into(),
            role: Role::Cajero,
            is_active: true,
            failed_attempts: 0,
        };
        for _ in 0..MAX_INTENTOS_PIN {
            assert!(!u.bloqueado(MAX_INTENTOS_PIN));
            u.failed_attempts += 1;
        }
        assert!(u.bloqueado(MAX_INTENTOS_PIN));
    }

    #[test]
    fn los_roles_viajan_por_texto() {
        assert_eq!(Role::desde_texto("admin"), Role::Admin);
        assert_eq!(Role::desde_texto("Cajero"), Role::Cajero);
        assert_eq!(Role::desde_texto("algo raro"), Role::Cajero);
        assert_eq!(Role::Supervisor.as_str(), "supervisor");
    }
}
