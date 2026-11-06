//! Facturación electrónica visto desde el punto de venta.
//!
//! Aquí se junta lo que ya existe (las ventas guardadas) con el armado del
//! comprobante ([`pos_fiscal`]), y se decide qué pasa cuando el PAC no
//! contesta. La regla es una y conviene saberla de memoria:
//!
//! > **La factura se genera y se guarda siempre, se timbre o no.**
//!
//! Si el timbre falla, queda en la cola con su XML y se reintenta después. Un
//! punto de venta que pierde una factura porque se cayó el internet le cuesta
//! dinero al negocio; uno que la guarda y la timbra dos horas después, no.

use std::fs;
use std::path::Path;

use pos_core::{ahora_segs, FechaHora};
use pos_db::clientes::Clientes;
use pos_db::rusqlite::{params, Connection};

use pos_fiscal::cancelacion;
use pos_fiscal::cfdi::{Comprobante, Emisor, Receptor, Relacion, TasaIva};
use pos_fiscal::global::{self, LineaTicket, Ticket};
use pos_fiscal::pac::{
    mensaje_http_bloqueado, parece_url_http, Pac, PacCarpeta, PacDePrueba, PacHttp,
};

use crate::dto::{ConfigFiscalDto, FacturaDto};

// --- configuración: vive en `settings`, como todo lo demás ------------------

/// (clave en `settings`, de dónde sacar el valor).
type CampoConfig = (&'static str, fn(&ConfigFiscalDto) -> &str);

const CLAVES: [CampoConfig; 8] = [
    ("fiscal_rfc", |c| &c.rfc),
    ("fiscal_nombre", |c| &c.nombre),
    ("fiscal_regimen", |c| &c.regimen_fiscal),
    ("fiscal_cp", |c| &c.codigo_postal),
    ("fiscal_forma_pago", |c| &c.forma_pago),
    ("fiscal_periodicidad", |c| &c.periodicidad),
    ("fiscal_carpeta_salida", |c| &c.carpeta_salida),
    ("fiscal_carpeta_entrada", |c| &c.carpeta_entrada),
];

fn leer(conn: &Connection, clave: &str) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![clave],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_default()
}

fn escribir(conn: &Connection, clave: &str, valor: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![clave, valor],
    )
    .map_err(|e| format!("No pude guardar {clave}: {e}"))?;
    Ok(())
}

pub fn leer_config(conn: &Connection) -> ConfigFiscalDto {
    ConfigFiscalDto {
        rfc: leer(conn, "fiscal_rfc"),
        nombre: leer(conn, "fiscal_nombre"),
        regimen_fiscal: leer(conn, "fiscal_regimen"),
        codigo_postal: leer(conn, "fiscal_cp"),
        forma_pago: if leer(conn, "fiscal_forma_pago").is_empty() {
            "01".to_string()
        } else {
            leer(conn, "fiscal_forma_pago")
        },
        periodicidad: if leer(conn, "fiscal_periodicidad").is_empty() {
            "01".to_string()
        } else {
            leer(conn, "fiscal_periodicidad")
        },
        carpeta_salida: leer(conn, "fiscal_carpeta_salida"),
        carpeta_entrada: leer(conn, "fiscal_carpeta_entrada"),
    }
}

pub fn guardar_config(conn: &Connection, c: &ConfigFiscalDto) -> Result<(), String> {
    rechazar_url_http(c)?;
    // Se guarda tal cual lo escribió la persona, sin normalizar: si escribió
    // "xaXX010101000" hay que decírselo, no corregirlo a escondidas.
    for (clave, obtener) in CLAVES {
        escribir(conn, clave, obtener(c))?;
    }
    Ok(())
}

/// Revisa lo que se puede revisar antes de gastar un timbre.
///
/// No pretende ser un validador del SAT: es la red que atrapa el error de
/// dedo (un RFC de 11 letras, un código postal de 4 dígitos) y lo explica en
/// palabras de la tienda, no con una clave de error.
pub fn revisar_config(c: &ConfigFiscalDto) -> Result<(), String> {
    let rfc = c.rfc.trim().to_uppercase();
    if rfc.len() != 12 && rfc.len() != 13 {
        return Err(format!(
            "El RFC debe tener 12 o 13 caracteres y tiene {}. Escríbelo como aparece en tu constancia de situacion fiscal.",
            rfc.len()
        ));
    }
    if c.nombre.trim().is_empty() {
        return Err("Falta el nombre o la razón social de quien factura.".into());
    }
    if !c
        .regimen_fiscal
        .trim()
        .chars()
        .all(|ch| ch.is_ascii_digit())
        || c.regimen_fiscal.trim().len() != 3
    {
        return Err(
            "El régimen fiscal son tres números (por ejemplo 626). Viene en tu constancia.".into(),
        );
    }
    if !c.codigo_postal.trim().chars().all(|ch| ch.is_ascii_digit())
        || c.codigo_postal.trim().len() != 5
    {
        return Err("El código postal son cinco números.".into());
    }
    rechazar_url_http(c)?;
    Ok(())
}

/// La URL que alguien escribió donde iba una carpeta, si es HTTP.
pub fn url_http_en(c: &ConfigFiscalDto) -> Option<String> {
    for s in [&c.carpeta_salida, &c.carpeta_entrada] {
        if parece_url_http(s) {
            return Some(s.trim().to_string());
        }
    }
    None
}

fn rechazar_url_http(c: &ConfigFiscalDto) -> Result<(), String> {
    match url_http_en(c) {
        Some(url) => Err(mensaje_http_bloqueado(&url)),
        None => Ok(()),
    }
}

fn emisor(c: &ConfigFiscalDto) -> Emisor {
    Emisor {
        rfc: c.rfc.trim().to_uppercase(),
        nombre: c.nombre.trim().to_uppercase(),
        regimen_fiscal: c.regimen_fiscal.trim().to_string(),
        codigo_postal: c.codigo_postal.trim().to_string(),
    }
}

/// Puntos base (1600 = 16%) a la tasa que entiende el comprobante.
pub fn tasa_de(bp: i64) -> TasaIva {
    match bp {
        1600 => TasaIva::Dieciseis,
        800 => TasaIva::Ocho,
        0 => TasaIva::Cero,
        // Una tasa que no conocemos se trata como exenta antes que inventar
        // un número y mandar un IVA que nadie cobró.
        _ => TasaIva::Exento,
    }
}

// --- la factura global del día ----------------------------------------------

/// Junta las ventas de un día en un solo comprobante y lo deja en la cola.
///
/// `fecha` va en formato `AAAA-MM-DD` y se interpreta como **día civil de la
/// tienda**, no como día UTC: una venta de las 7 de la noche es de ese día,
/// aunque en UTC ya sea mañana.
pub fn factura_global(
    conn: &Connection,
    fecha: &str,
    periodicidad: &str,
    usuario: Option<i64>,
) -> Result<FacturaDto, String> {
    let config = leer_config(conn);
    revisar_config(&config)?;

    // Sólo entran las ventas que nadie ha facturado. Una venta ya metida en
    // otra factura global no se cuenta dos veces: eso sería pagar IVA dos
    // veces sobre lo mismo.
    let mut q = conn
        .prepare(
            "SELECT s.id, s.folio FROM sales s
             WHERE date(s.created_at, '-6 hours') = ?1
               AND s.status = 'completada'
               AND s.fiscal_status IN ('no_requerido', 'pendiente')
             ORDER BY s.id",
        )
        .map_err(|e| format!("No pude consultar las ventas: {e}"))?;
    let ventas: Vec<(i64, String)> = q
        .query_map(params![fecha], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(q);

    if ventas.is_empty() {
        return Err(format!(
            "No hay ventas pendientes de facturar el {fecha}. \
             Puede que ya se hayan facturado o que ese día no se vendió."
        ));
    }

    let mut tickets = Vec::with_capacity(ventas.len());
    for (id, folio) in &ventas {
        let mut qi = conn
            .prepare(
                "SELECT si.sku, si.description, si.qty_millis, si.unit_price_cents,
                        si.discount_cents, si.tax_bp,
                        COALESCE(p.clave_sat, '01010101'),
                        COALESCE(p.clave_unidad, 'H87'),
                        COALESCE(p.unidad_sat, 'Pieza')
                 FROM sale_items si
                 LEFT JOIN products p ON p.sku = si.sku
                 WHERE si.sale_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let lineas: Vec<LineaTicket> = qi
            .query_map(params![id], |r| {
                Ok(LineaTicket {
                    sku: r.get::<_, String>(0)?,
                    descripcion: r.get::<_, String>(1)?,
                    cantidad_millis: r.get::<_, i64>(2)?,
                    precio_bruto_cents: r.get::<_, i64>(3)?,
                    descuento_cents: r.get::<_, i64>(4)?,
                    tasa: tasa_de(r.get::<_, i64>(5)?),
                    clave_sat: r.get::<_, String>(6).unwrap_or_default(),
                    clave_unidad: r.get::<_, String>(7).unwrap_or_default(),
                    unidad: r.get::<_, String>(8).unwrap_or_default(),
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(qi);
        if lineas.is_empty() {
            continue;
        }
        tickets.push(Ticket {
            folio: folio.clone(),
            lineas,
        });
    }

    // La hora de expedición: el final del día que se está facturando.
    let fecha_xml = format!("{fecha}T23:59:00");
    let (meses, anio) = meses_y_anio(fecha, periodicidad);
    let comprobante = global::desde_tickets(
        &emisor(&config),
        &tickets,
        &fecha_xml,
        &config.forma_pago,
        periodicidad,
        &meses,
        &anio,
    );
    let xml = pos_fiscal::cfdi::xml(&comprobante);
    let folios: Vec<String> = tickets.iter().map(|t| t.folio.clone()).collect();

    let folio = comprobante.folio.clone();
    conn.execute(
        "INSERT INTO facturas
            (folio, tipo, periodo, periodicidad, receptor_rfc, receptor_nombre,
             receptor_cp, receptor_regimen, uso_cfdi,
             subtotal_cents, iva_cents, total_cents, ventas, estado, xml, creado_por)
         VALUES (?1, 'global', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 'pendiente', ?13, ?14)",
        params![
            folio,
            fecha,
            periodicidad,
            comprobante.receptor.rfc,
            comprobante.receptor.nombre,
            comprobante.receptor.codigo_postal,
            comprobante.receptor.regimen_fiscal,
            comprobante.receptor.uso_cfdi,
            comprobante.subtotal_cents(),
            comprobante.iva_cents(),
            comprobante.total_cents(),
            folios.join(","),
            xml,
            usuario,
        ],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            format!("El {fecha} ya se facturó. Si quieres volver a hacerlo, cancela primero la factura anterior.")
        } else {
            format!("No pude guardar la factura: {e}")
        }
    })?;
    let id = conn.last_insert_rowid();

    // Las ventas quedan marcadas: aunque el timbre falle, ya no se vuelven a
    // meter en otra factura. Eso evita el duplicado silencioso.
    for t in &tickets {
        conn.execute(
            "UPDATE sales SET fiscal_status = 'pendiente' WHERE folio = ?1",
            params![t.folio],
        )
        .map_err(|e| e.to_string())?;
    }

    // Y el XML va a la carpeta de salida, si se configuró una.
    dejar_xml(&config, &xml);

    leer_factura(conn, id)
}

/// Factura individual: una venta a nombre de un cliente con RFC.
///
/// Ésta es la que se le da a quien la pide, y por eso **esa venta no se mete
/// en la factura global del día**: si se metiera, el SAT vería el mismo ingreso
/// dos veces. La venta ya se marcó como `requerida` al cobrar, así que la
/// global la deja fuera sola.
pub fn facturar_venta(
    conn: &Connection,
    folio: &str,
    fecha: &str,
    usuario: Option<i64>,
) -> Result<FacturaDto, String> {
    let config = leer_config(conn);
    revisar_config(&config)?;

    let (venta_id, cliente_id, estado_fiscal): (i64, Option<i64>, String) = conn
        .query_row(
            "SELECT id, customer_id, fiscal_status FROM sales WHERE folio = ?1",
            params![folio.trim()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| format!("No encontré la venta {folio}."))?;

    if estado_fiscal == "facturada" || estado_fiscal == "timbrada" {
        return Err(format!(
            "La venta {folio} ya está facturada. Si la factura salió mal, cancélala \
             primero o hazle una nota de crédito."
        ));
    }

    // Los datos del cliente: sin los cuatro, no hay factura a nombre de nadie.
    let cliente = match cliente_id {
        Some(id) => Clientes::por_id(conn, id).map_err(|e| e.to_string())?,
        None => {
            return Err(format!(
                "La venta {folio} fue a público en general, y esas van en la factura \
                 global del día. Sólo se facturan aparte las ventas de clientes con RFC."
            ))
        }
    };
    let Some(cliente) = cliente else {
        return Err("No encontré al cliente de esa venta.".into());
    };
    let rfc = cliente.rfc.trim().to_uppercase();
    let mut faltan = Vec::new();
    if rfc.is_empty() || rfc == pos_fiscal::cfdi::RFC_PUBLICO {
        faltan.push("su RFC");
    }
    if cliente.regimen_fiscal.trim().is_empty() {
        faltan.push("su régimen fiscal");
    }
    if cliente.codigo_postal.trim().len() != 5 {
        faltan.push("el código postal de su domicilio fiscal (5 dígitos)");
    }
    if !faltan.is_empty() {
        return Err(format!(
            "A {} le falta {} para poder facturarle. Se captura en Clientes.",
            cliente.name,
            faltan.join(", ")
        ));
    }

    // Los renglones, con las claves del SAT de cada producto.
    let mut qi = conn
        .prepare(
            "SELECT si.sku, si.description, si.qty_millis, si.unit_price_cents,
                si.discount_cents, si.tax_bp,
                COALESCE(p.clave_sat, '01010101'),
                COALESCE(p.clave_unidad, 'H87'),
                COALESCE(p.unidad_sat, 'Pieza')
         FROM sale_items si
         LEFT JOIN products p ON p.sku = si.sku
         WHERE si.sale_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let lineas: Vec<LineaTicket> = qi
        .query_map(params![venta_id], |r| {
            Ok(LineaTicket {
                sku: r.get::<_, String>(0)?,
                descripcion: r.get::<_, String>(1)?,
                cantidad_millis: r.get::<_, i64>(2)?,
                precio_bruto_cents: r.get::<_, i64>(3)?,
                descuento_cents: r.get::<_, i64>(4)?,
                tasa: tasa_de(r.get::<_, i64>(5)?),
                clave_sat: r.get::<_, String>(6).unwrap_or_default(),
                clave_unidad: r.get::<_, String>(7).unwrap_or_default(),
                unidad: r.get::<_, String>(8).unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(qi);
    if lineas.is_empty() {
        return Err(format!("La venta {folio} no tiene renglones."));
    }
    let ticket = Ticket {
        folio: folio.trim().to_string(),
        lineas,
    };

    // Cómo pagó: si le fiaron, es PPD (pago diferido); si no, PUE.
    let metodo: String = conn
        .query_row(
            "SELECT method FROM payments WHERE sale_id = ?1 ORDER BY amount_cents DESC LIMIT 1",
            params![venta_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "efectivo".to_string());
    let (metodo_pago, forma_pago) = if metodo.contains("credito") || metodo.contains("crédito") {
        ("PPD".to_string(), "99".to_string())
    } else {
        ("PUE".to_string(), forma_pago_sat(&metodo))
    };

    let receptor = Receptor {
        rfc: rfc.clone(),
        nombre: cliente.name.clone(),
        codigo_postal: cliente.codigo_postal.trim().to_string(),
        regimen_fiscal: cliente.regimen_fiscal.trim().to_string(),
        uso_cfdi: if cliente.uso_cfdi.trim().is_empty() {
            "S01".to_string()
        } else {
            cliente.uso_cfdi.trim().to_uppercase()
        },
    };

    let mut comprobante = Comprobante::ingreso(
        "F",
        &folio.trim().to_uppercase(),
        &format!("{fecha}T12:00:00"),
        emisor(&config),
        receptor,
    );
    comprobante.forma_pago = forma_pago;
    // PPD = pago en parcialidades o diferido: la factura queda a deber y
    // necesita un complemento cuando el cliente pague.
    let es_credito = metodo_pago == "PPD";
    comprobante.metodo_pago = metodo_pago.clone();
    for concepto in global::conceptos_por_linea(&ticket) {
        comprobante.agregar(concepto);
    }
    let xml = pos_fiscal::cfdi::xml(&comprobante);

    conn.execute(
        "INSERT INTO facturas
            (folio, tipo, periodo, periodicidad, receptor_rfc, receptor_nombre,
             receptor_cp, receptor_regimen, uso_cfdi,
             subtotal_cents, iva_cents, total_cents, ventas, estado, xml, creado_por,
             metodo_pago, saldo_cents)
         VALUES (?1, 'individual', ?2, '', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                 'pendiente', ?12, ?13, ?14, ?15)",
        params![
            comprobante.folio,
            fecha,
            comprobante.receptor.rfc,
            comprobante.receptor.nombre,
            comprobante.receptor.codigo_postal,
            comprobante.receptor.regimen_fiscal,
            comprobante.receptor.uso_cfdi,
            comprobante.subtotal_cents(),
            comprobante.iva_cents(),
            comprobante.total_cents(),
            folio.trim(),
            xml,
            usuario,
            metodo_pago,
            // Si se fue a crédito, la factura queda debiendo todo: ese saldo
            // es el que el complemento de pagos tiene que ir cubriendo.
            if es_credito {
                comprobante.total_cents()
            } else {
                0
            },
        ],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            format!("La venta {folio} ya tiene una factura individual.")
        } else {
            format!("No pude guardar la factura: {e}")
        }
    })?;
    let id = conn.last_insert_rowid();

    // Ya no entra en la global del día.
    conn.execute(
        "UPDATE sales SET fiscal_status = 'facturada' WHERE id = ?1",
        params![venta_id],
    )
    .map_err(|e| format!("No pude marcar la venta como facturada: {e}"))?;

    dejar_xml(&config, &xml);
    leer_factura(conn, id)
}

/// Devuelve un **monto** en vez de una cantidad de piezas.
///
/// Sirve cuando las piezas no alcanzan para describir la devolución: ropa,
/// a granel, o simplemente "te devuelvo $50 de lo que te compré".
///
/// El monto se reparte entre los renglones de la venta en proporción a lo que
/// valía cada uno, y el reparto se hace por **residuo mayor** para que la suma
/// sea exacta: repartir con redondeo normal pierde un centavo casi siempre.
///
/// La nota cubre lo que de veras sale de la caja, así que el total puede
/// distar uno o dos centavos del monto pedido si las piezas no reparten
/// exacto. Ese centavo no se inventa ni se pierde: se reporta.
pub fn nota_credito_monto(
    conn: &Connection,
    folio: &str,
    fecha: &str,
    monto_cents: i64,
    usuario: Option<i64>,
) -> Result<(FacturaDto, i64), String> {
    if monto_cents <= 0 {
        return Err("El monto a devolver debe ser mayor a cero.".into());
    }

    // Cuánto queda vivo de cada renglón (lo vendido menos lo ya devuelto).
    let vendidas = lineas_de_la_venta(conn, folio)?;
    let ya_devuelto = pos_db::products::devuelto_por_notas(conn, folio.trim()).unwrap_or_default();
    let mut vivos: Vec<(LineaTicket, i64)> = Vec::new(); // (línea completa, millis vivos)
    for l in &vendidas {
        let devueltos: i64 = ya_devuelto
            .iter()
            .filter(|(s, _)| s == &l.sku)
            .map(|(_, m)| m)
            .sum();
        let vivos_millis = (l.cantidad_millis - devueltos).max(0);
        if vivos_millis > 0 {
            vivos.push((l.clone(), vivos_millis));
        }
    }
    if vivos.is_empty() {
        return Err(format!(
            "La venta {folio} ya está devuelta por completo. No hay nada que abonar."
        ));
    }

    // Cuánto dinero representa cada renglón vivo.
    let valor_vivo: Vec<i64> = vivos
        .iter()
        .map(|(l, millis)| l.parcial(*millis).bruto_cents())
        .collect();
    let disponible: i64 = valor_vivo.iter().sum();
    if monto_cents > disponible {
        return Err(format!(
            "De la venta {folio} quedan {} vivos, no alcanza para devolver {}.",
            pos_core::Money::from_cents(disponible).format_mxn(),
            pos_core::Money::from_cents(monto_cents).format_mxn()
        ));
    }

    // Reparto proporcional por residuo mayor: la suma da el monto exacto.
    let mut centavos: Vec<i64> = valor_vivo
        .iter()
        .map(|v| (monto_cents * v) / disponible)
        .collect();
    let mut residuos: Vec<(i64, usize)> = valor_vivo
        .iter()
        .enumerate()
        .map(|(i, v)| ((monto_cents * v) % disponible, i))
        .collect();
    residuos.sort_by_key(|r| std::cmp::Reverse(r.0));
    let repartido: i64 = centavos.iter().sum();
    let mut faltan = monto_cents - repartido;
    for (_, i) in residuos {
        if faltan <= 0 {
            break;
        }
        centavos[i] += 1;
        faltan -= 1;
    }

    // Los centavos de cada renglón se convierten a cantidad, que es lo que la
    // nota tiene que decir (el SAT no acepta "una devolución de $18.00").
    let mut devoluciones: Vec<(String, i64)> = Vec::new();
    for ((l, millis_vivos), centavos_linea) in vivos.iter().zip(centavos) {
        let precio = l.precio_bruto_cents.max(1);
        let millis = (centavos_linea * 1_000 / precio).clamp(0, *millis_vivos);
        if millis > 0 {
            devoluciones.push((l.sku.clone(), millis));
        }
    }
    if devoluciones.is_empty() {
        return Err(format!(
            "Con {} no alcanza ni para una pieza de lo que se vendió en {folio}.",
            pos_core::Money::from_cents(monto_cents).format_mxn()
        ));
    }

    let nota = nota_credito(conn, folio, fecha, &devoluciones, usuario)?;
    Ok((nota, monto_cents))
}

/// Los renglones de una venta, tal cual se vendieron.
fn lineas_de_la_venta(conn: &Connection, folio: &str) -> Result<Vec<LineaTicket>, String> {
    let venta_id: i64 = conn
        .query_row(
            "SELECT id FROM sales WHERE folio = ?1",
            params![folio.trim()],
            |r| r.get(0),
        )
        .map_err(|_| format!("No encontré la venta {folio}."))?;
    let mut qi = conn
        .prepare(
            "SELECT si.sku, si.description, si.qty_millis, si.unit_price_cents,
                    si.discount_cents, si.tax_bp,
                    COALESCE(p.clave_sat, '01010101'),
                    COALESCE(p.clave_unidad, 'H87'),
                    COALESCE(p.unidad_sat, 'Pieza')
             FROM sale_items si
             LEFT JOIN products p ON p.sku = si.sku
             WHERE si.sale_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let lineas = qi
        .query_map(params![venta_id], |r| {
            Ok(LineaTicket {
                sku: r.get::<_, String>(0)?,
                descripcion: r.get::<_, String>(1)?,
                cantidad_millis: r.get::<_, i64>(2)?,
                precio_bruto_cents: r.get::<_, i64>(3)?,
                descuento_cents: r.get::<_, i64>(4)?,
                tasa: tasa_de(r.get::<_, i64>(5)?),
                clave_sat: r.get::<_, String>(6).unwrap_or_default(),
                clave_unidad: r.get::<_, String>(7).unwrap_or_default(),
                unidad: r.get::<_, String>(8).unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(lineas)
}

/// Emite el complemento de pagos de lo que un cliente acaba de pagar.
///
/// Cuando se le facturó a crédito (PPD), la factura dice "te debo". Este
/// comprobante dice "ya pagué", y va ligado por UUID a las facturas que está
/// cubriendo. Sin él, el cliente tiene una factura que no puede deducir.
///
/// El pago se reparte entre las facturas pendientes del cliente, de la más
/// vieja a la más nueva, que es como se hace normalmente.
pub fn complemento_pago(
    conn: &Connection,
    cliente_id: i64,
    monto_cents: i64,
    fecha: &str,
    forma_pago: &str,
    usuario: Option<i64>,
) -> Result<FacturaDto, String> {
    if monto_cents <= 0 {
        return Err("El monto del pago debe ser mayor a cero.".into());
    }
    let config = leer_config(conn);
    revisar_config(&config)?;

    let cliente = Clientes::por_id(conn, cliente_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No encontré a ese cliente.".to_string())?;
    let rfc = cliente.rfc.trim().to_uppercase();
    if rfc.is_empty() || rfc == pos_fiscal::cfdi::RFC_PUBLICO {
        return Err(format!(
            "{} no tiene RFC, así que no se le puede emitir un complemento de pagos.",
            cliente.name
        ));
    }
    if cliente.regimen_fiscal.trim().is_empty() || cliente.codigo_postal.trim().len() != 5 {
        return Err(format!(
            "A {} le faltan datos fiscales (régimen o código postal).",
            cliente.name
        ));
    }

    // Las facturas a crédito que siguen debiendo, de la más vieja a la nueva.
    let mut q = conn
        .prepare(
            "SELECT id, COALESCE(uuid, ''), saldo_cents, total_cents, iva_cents
             FROM facturas
             WHERE tipo = 'individual' AND metodo_pago = 'PPD'
               AND estado = 'timbrada' AND saldo_cents > 0
               AND receptor_rfc = ?1
             ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let deudas: Vec<(i64, String, i64, i64, i64)> = q
        .query_map(params![rfc], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(q);
    if deudas.is_empty() {
        return Err(format!(
            "{} no tiene facturas a crédito pendientes de pago.",
            cliente.name
        ));
    }

    let debiendo: i64 = deudas.iter().map(|d| d.2).sum();
    if monto_cents > debiendo {
        return Err(format!(
            "{} debe {} y estás pagando {}. Sobra dinero.",
            cliente.name,
            pos_core::Money::from_cents(debiendo).format_mxn(),
            pos_core::Money::from_cents(monto_cents).format_mxn()
        ));
    }

    // Se reparte empezando por la factura más vieja.
    let mut falta = monto_cents;
    let mut documentos: Vec<(i64, pos_fiscal::pago::DocumentoRelacionado)> = Vec::new();
    for (factura_id, uuid, saldo, total, iva) in deudas {
        if falta <= 0 {
            break;
        }
        let pagado = falta.min(saldo);
        let parcialidad: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pagos_aplicados WHERE factura_id = ?1",
                params![factura_id],
                |r| r.get(0),
            )
            .unwrap_or(0)
            + 1;
        documentos.push((
            factura_id,
            pos_fiscal::pago::DocumentoRelacionado {
                uuid: uuid.clone(),
                num_parcialidad: parcialidad,
                saldo_anterior_cents: saldo,
                pagado_cents: pagado,
                saldo_insoluto_cents: saldo - pagado,
                tasa: TasaIva::Dieciseis,
                iva_de_la_factura_cents: iva,
                total_de_la_factura_cents: total,
            },
        ));
        falta -= pagado;
    }

    let receptor = Receptor {
        rfc: rfc.clone(),
        nombre: cliente.name.clone(),
        codigo_postal: cliente.codigo_postal.trim().to_string(),
        regimen_fiscal: cliente.regimen_fiscal.trim().to_string(),
        uso_cfdi: if cliente.uso_cfdi.trim().is_empty() {
            "S01".to_string()
        } else {
            cliente.uso_cfdi.trim().to_uppercase()
        },
    };

    // El folio nace del id, así que el XML se arma después de insertar.
    conn.execute(
        "INSERT INTO facturas
            (folio, tipo, periodo, periodicidad, receptor_rfc, receptor_nombre,
             receptor_cp, receptor_regimen, uso_cfdi,
             subtotal_cents, iva_cents, total_cents, ventas, estado, xml, creado_por,
             metodo_pago, saldo_cents)
         VALUES ('', 'pago', ?1, '', ?2, ?3, ?4, ?5, ?6, 0, 0, 0, '', 'pendiente',
                 '', ?7, 'PPD', 0)",
        params![
            fecha,
            rfc,
            cliente.name,
            cliente.codigo_postal,
            cliente.regimen_fiscal,
            receptor.uso_cfdi.clone(),
            usuario,
        ],
    )
    .map_err(|e| format!("No pude guardar el complemento: {e}"))?;
    let id = conn.last_insert_rowid();
    let folio = format!("CP-{id}");

    let pago = pos_fiscal::pago::Pago {
        fecha: format!("{fecha}T12:00:00"),
        forma_pago: forma_pago.trim().to_string(),
        moneda: "MXN".to_string(),
        monto_cents,
        num_operacion: String::new(),
        documentos: documentos.iter().map(|(_, d)| d.clone()).collect(),
    };
    let comprobante = pos_fiscal::pago::ComprobantePago {
        serie: "CP".into(),
        folio: id.to_string(),
        fecha: format!("{fecha}T12:00:00"),
        lugar_expedicion: config.codigo_postal.clone(),
        emisor_rfc: config.rfc.clone(),
        emisor_nombre: config.nombre.clone(),
        emisor_regimen: config.regimen_fiscal.clone(),
        receptor,
        pagos: vec![pago],
    };
    let xml = pos_fiscal::pago::xml(&comprobante);
    conn.execute(
        "UPDATE facturas SET xml = ?1, folio = ?2 WHERE id = ?3",
        params![xml, folio, id],
    )
    .map_err(|e| e.to_string())?;

    // Se descuenta de cada factura y se anota el número de parcialidad.
    for (factura_id, doc) in &documentos {
        conn.execute(
            "UPDATE facturas SET saldo_cents = saldo_cents - ?1 WHERE id = ?2",
            params![doc.pagado_cents, factura_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO pagos_aplicados (pago_id, factura_id, monto_cents)
             VALUES (?1, ?2, ?3)",
            params![id, factura_id, doc.pagado_cents],
        )
        .map_err(|e| e.to_string())?;
    }

    dejar_xml(&config, &xml);
    leer_factura(conn, id)
}

/// Cuánto siguen debiendo las facturas a crédito de un cliente.
pub fn saldo_por_pagar(conn: &Connection, cliente_id: i64) -> Result<i64, String> {
    let cliente = Clientes::por_id(conn, cliente_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No encontré a ese cliente.".to_string())?;
    conn.query_row(
        // Cuenta todo lo que se facturó a crédito y no se ha saldado, esté
        // timbrada o no: la deuda existe desde que se emitió la factura.
        "SELECT COALESCE(SUM(saldo_cents), 0) FROM facturas
         WHERE tipo = 'individual' AND metodo_pago = 'PPD' AND estado != 'cancelada'
           AND receptor_rfc = ?1",
        params![cliente.rfc.trim().to_uppercase()],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// La clave de `c_FormaPago` según cómo pagó el cliente.
fn forma_pago_sat(metodo: &str) -> String {
    match metodo.trim().to_lowercase().as_str() {
        "efectivo" => "01".to_string(),
        "transferencia" => "03".to_string(),
        "tarjeta" => "04".to_string(),
        // Vales y fiado: el SAT tiene sus propias claves, pero dependen del
        // tipo exacto de vale. "99" (por definir) es honesto; inventar otra
        // sería peor.
        _ => "99".to_string(),
    }
}

/// Cuántas ventas con RFC siguen esperando su factura.
///
/// Sirve para que la pantalla avise: esas ventas NO van en la global, así que
/// si nadie las factura, ese ingreso se queda sin reportar.
pub fn ventas_por_facturar(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM sales WHERE fiscal_status = 'requerida'",
        [],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// Nota de crédito de una venta ya facturada.
///
/// La devolución existe desde antes en la caja; lo que faltaba era el
/// comprobante que le dice al SAT "de aquella factura, quítale esto". Por eso
/// la nota va **ligada por UUID** a la factura que la originó.
///
/// Si la venta todavía no se ha facturado no hay nada que restar: se dice con
/// todas sus letras en vez de fabricar una nota huérfana que el SAT rechazaría.
pub fn nota_credito(
    conn: &Connection,
    folio: &str,
    fecha: &str,
    devoluciones: &[(String, i64)],
    usuario: Option<i64>,
) -> Result<FacturaDto, String> {
    let config = leer_config(conn);
    revisar_config(&config)?;

    // 1) ¿De qué factura timbrada salió esta venta?
    let (factura_id, uuid, receptor_rfc, receptor_nombre, receptor_cp, receptor_regimen, uso_cfdi): (
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT id, COALESCE(uuid, ''), receptor_rfc, receptor_nombre,
                    receptor_cp, receptor_regimen, uso_cfdi
             FROM facturas
             WHERE estado = 'timbrada'
               AND (',' || ventas || ',') LIKE ('%,' || ?1 || ',%')
             ORDER BY id DESC LIMIT 1",
            params![folio.trim()],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .map_err(|_| {
            format!(
                "La venta {folio} todavía no está facturada, así que no se le puede hacer \
                 nota de crédito. Factura el día primero y vuelve a intentar."
            )
        })?;

    // 2) Las líneas que se devuelven: las mismas que se vendieron.
    let venta_id: i64 = conn
        .query_row(
            "SELECT id FROM sales WHERE folio = ?1",
            params![folio.trim()],
            |r| r.get(0),
        )
        .map_err(|_| format!("No encontré la venta {folio}."))?;
    let mut qi = conn
        .prepare(
            "SELECT si.sku, si.description, si.qty_millis, si.unit_price_cents,
                    si.discount_cents, si.tax_bp,
                    COALESCE(p.clave_sat, '01010101'),
                    COALESCE(p.clave_unidad, 'H87'),
                    COALESCE(p.unidad_sat, 'Pieza')
             FROM sale_items si
             LEFT JOIN products p ON p.sku = si.sku
             WHERE si.sale_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let lineas: Vec<LineaTicket> = qi
        .query_map(params![venta_id], |r| {
            Ok(LineaTicket {
                sku: r.get::<_, String>(0)?,
                descripcion: r.get::<_, String>(1)?,
                cantidad_millis: r.get::<_, i64>(2)?,
                precio_bruto_cents: r.get::<_, i64>(3)?,
                descuento_cents: r.get::<_, i64>(4)?,
                tasa: tasa_de(r.get::<_, i64>(5)?),
                clave_sat: r.get::<_, String>(6).unwrap_or_default(),
                clave_unidad: r.get::<_, String>(7).unwrap_or_default(),
                unidad: r.get::<_, String>(8).unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(qi);
    if lineas.is_empty() {
        return Err(format!("La venta {folio} no tiene renglones."));
    }
    // Lo que ya se devolvió en notas anteriores: si no se descuenta, una
    // segunda nota parcial volvería a devolver las mismas dos cocas.
    let ya_devuelto = pos_db::products::devuelto_por_notas(conn, folio.trim()).unwrap_or_default();
    let disponible_de = |sku: &str, vendida: i64| -> i64 {
        let devuelta: i64 = ya_devuelto
            .iter()
            .filter(|(s, _)| s == sku)
            .map(|(_, m)| m)
            .sum();
        (vendida - devuelta).max(0)
    };

    let mut lineas = lineas;
    if devoluciones.is_empty() {
        // Devolución total: todo lo que quede sin devolver.
        let mut nuevas = Vec::new();
        for l in &lineas {
            let cuanto = disponible_de(&l.sku, l.cantidad_millis);
            if cuanto > 0 {
                nuevas.push(l.parcial(cuanto));
            }
        }
        if nuevas.is_empty() {
            return Err(format!(
                "La venta {folio} ya está devuelta por completo. No hay nada que abonar."
            ));
        }
        lineas = nuevas;
    } else {
        let mut nuevas = Vec::new();
        for (sku, cantidad) in devoluciones {
            let vendida = match lineas.iter().find(|l| l.sku == *sku) {
                Some(l) => l,
                None => return Err(format!("La venta {folio} no vendió {sku}.")),
            };
            if *cantidad <= 0 {
                return Err(format!(
                    "La cantidad a devolver de {sku} debe ser mayor a cero."
                ));
            }
            let disponible = disponible_de(sku, vendida.cantidad_millis);
            if *cantidad > disponible {
                return Err(format!(
                    "De {sku} la venta {folio} llevaba {vendida} y ya se devolvieron {ya}. \
                     Sólo quedan {disponible} por devolver.",
                    vendida = vendida.cantidad_millis,
                    ya = vendida.cantidad_millis - disponible,
                ));
            }
            nuevas.push(vendida.parcial(*cantidad));
        }
        lineas = nuevas;
    }

    let ticket = Ticket {
        folio: folio.trim().to_string(),
        lineas: lineas.clone(),
    };

    // 3) El receptor se reconstruye de la factura original, sea público en
    //    general o un cliente con RFC: la nota tiene que ir a nombre de la
    //    misma persona que recibió la factura.
    let receptor = if receptor_rfc.trim().to_uppercase() == pos_fiscal::cfdi::RFC_PUBLICO {
        Receptor::publico_general(&config.codigo_postal)
    } else {
        Receptor {
            rfc: receptor_rfc.trim().to_uppercase(),
            nombre: receptor_nombre.clone(),
            codigo_postal: receptor_cp.clone(),
            regimen_fiscal: receptor_regimen.clone(),
            uso_cfdi: uso_cfdi.clone(),
        }
    };

    let comprobante = global::nota_credito(
        &emisor(&config),
        &receptor,
        &ticket,
        &format!("{fecha}T12:00:00"),
        Relacion {
            tipo_relacion: "01".to_string(),
            uuids: vec![uuid],
        },
        &format!("NC-{factura_id}-{}", folio.trim()),
    );
    let xml = pos_fiscal::cfdi::xml(&comprobante);

    // 4) Se guarda. La llave única (egreso, NC, folio) evita que a la misma
    //    venta se le hagan dos notas por descuido.
    match conn.execute(
        "INSERT INTO facturas
            (folio, tipo, periodo, periodicidad, receptor_rfc, receptor_nombre,
             receptor_cp, receptor_regimen, uso_cfdi,
             subtotal_cents, iva_cents, total_cents, ventas, estado, xml, creado_por)
         VALUES (?1, 'egreso', ?2, 'NC', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                 'pendiente', ?12, ?13)",
        params![
            comprobante.folio,
            folio.trim(),
            receptor_rfc,
            receptor_nombre,
            receptor.codigo_postal,
            receptor.regimen_fiscal,
            receptor.uso_cfdi,
            comprobante.subtotal_cents(),
            comprobante.iva_cents(),
            comprobante.total_cents(),
            folio.trim(),
            xml,
            usuario,
        ],
    ) {
        Ok(_) => {}
        Err(e) if e.to_string().contains("UNIQUE") => {
            return Err(format!(
                "La venta {folio} ya tiene una nota de crédito. No se le puede hacer otra."
            ));
        }
        Err(e) => return Err(format!("No pude guardar la nota de crédito: {e}")),
    }
    let id = conn.last_insert_rowid();
    for l in &lineas {
        if let Err(e) = conn.execute(
            "INSERT INTO notas_lineas (factura_id, sku, cantidad_millis) VALUES (?1, ?2, ?3)",
            params![id, l.sku, l.cantidad_millis],
        ) {
            return Err(format!("No pude anotar lo devuelto: {e}"));
        }
    }
    dejar_xml(&config, &xml);
    leer_factura(conn, id)
}

/// Para el bimestre el SAT pide los dos meses ("01-02"); para el resto, uno.
fn meses_y_anio(fecha: &str, periodicidad: &str) -> (String, String) {
    let mes: String = fecha
        .chars()
        .filter(|c| c.is_ascii_digit())
        .skip(4)
        .take(2)
        .collect();
    let anio: String = fecha
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(4)
        .collect();
    if periodicidad == "05" {
        // Bimestre: enero-febrero es "01-02", marzo-abril "03-04", etc.
        let n: i64 = mes.parse().unwrap_or(1);
        let par = if n % 2 == 0 { n - 1 } else { n };
        (format!("{par:02}-{:02}", par + 1), anio)
    } else {
        (mes, anio)
    }
}

// --- la cola ----------------------------------------------------------------

fn leer_factura(conn: &Connection, id: i64) -> Result<FacturaDto, String> {
    conn.query_row(
        "SELECT id, folio, tipo, periodo, periodicidad, receptor_rfc,
                subtotal_cents, iva_cents, total_cents, ventas, estado, uuid,
                ultimo_error, intentos, created_at, timbrada_at
         FROM facturas WHERE id = ?1",
        params![id],
        |r| {
            Ok(FacturaDto {
                id: r.get(0)?,
                folio: r.get(1)?,
                tipo: r.get(2)?,
                periodo: r.get(3)?,
                periodicidad: r.get(4)?,
                receptor_rfc: r.get(5)?,
                subtotal_cents: r.get(6)?,
                iva_cents: r.get(7)?,
                total_cents: r.get(8)?,
                ventas: r.get(9)?,
                estado: r.get(10)?,
                uuid: r.get(11)?,
                ultimo_error: r.get(12)?,
                intentos: r.get(13)?,
                created_at: r.get(14)?,
                timbrada_at: r.get(15)?,
            })
        },
    )
    .map_err(|e| format!("No encontré la factura: {e}"))
}

pub fn listar(conn: &Connection, limite: usize) -> Result<Vec<FacturaDto>, String> {
    let mut q = conn
        .prepare("SELECT id FROM facturas ORDER BY id DESC LIMIT ?1")
        .map_err(|e| e.to_string())?;
    let ids: Vec<i64> = q
        .query_map(params![limite as i64], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(q);
    ids.iter().map(|id| leer_factura(conn, *id)).collect()
}

pub fn xml_de(conn: &Connection, id: i64) -> Result<String, String> {
    let f = leer_factura(conn, id)?;
    if f.estado == "cancelada" {
        let cancel: String = conn
            .query_row(
                "SELECT xml_cancelacion FROM facturas WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !cancel.trim().is_empty() {
            return Ok(cancel);
        }
    }
    if f.estado == "timbrada" {
        conn.query_row(
            "SELECT COALESCE(xml_timbrado, xml) FROM facturas WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    } else {
        conn.query_row("SELECT xml FROM facturas WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())
    }
}

/// Cancela una factura. Si ya estaba timbrada, deja la solicitud SAT en la
/// carpeta para el contador. Si aún no, sólo se baja de la cola.
pub fn cancelar(
    conn: &Connection,
    id: i64,
    motivo: &str,
    uuid_sustitucion: &str,
) -> Result<FacturaDto, String> {
    let f = leer_factura(conn, id)?;
    if f.estado == "cancelada" {
        return Err("Esa factura ya está cancelada.".into());
    }
    let config = leer_config(conn);
    let motivo = motivo.trim();
    let sust = uuid_sustitucion.trim();
    let mut xml_cancel = String::new();

    if f.estado == "timbrada" {
        let uuid = f
            .uuid
            .as_deref()
            .filter(|u| cancelacion::uuid_sat(u))
            .ok_or_else(|| {
                "Esa factura no trae UUID: no se puede cancelar ante el SAT.".to_string()
            })?;
        cancelacion::validar(motivo, sust)?;
        if let Some(url) = url_http_en(&config) {
            return Err(pos_fiscal::pac::mensaje_http_bloqueado(&url));
        }
        if config.carpeta_salida.trim().is_empty() {
            return Err(
                "Configura la carpeta «Por timbrar» para dejar la solicitud de cancelación.".into(),
            );
        }
        let h = FechaHora::cdmx(ahora_segs());
        let fecha_iso = format!(
            "{}T{:02}:{:02}:{:02}",
            h.fecha(),
            h.hora,
            h.minuto,
            h.segundo
        );
        xml_cancel = cancelacion::xml(&config.rfc, &fecha_iso, uuid, motivo, sust)?;
        dejar_cancelacion(&config, uuid, &xml_cancel)?;
    } else if !motivo.is_empty() {
        // Pendiente o rechazada: no hay SAT, pero el motivo queda anotado.
        if !matches!(motivo, "01" | "02" | "03" | "04") {
            return Err("El motivo de cancelación es 01, 02, 03 o 04 (catálogo del SAT).".into());
        }
    }

    conn.execute(
        "UPDATE facturas SET estado = 'cancelada', motivo_cancelacion = ?1,
                uuid_sustitucion = ?2, xml_cancelacion = ?3,
                cancelada_at = datetime('now'),
                ultimo_error = ?4
         WHERE id = ?5",
        params![
            motivo,
            sust,
            xml_cancel,
            format!("Cancelada ({motivo})"),
            id
        ],
    )
    .map_err(|e| e.to_string())?;

    // Las ventas vuelven a estar disponibles para facturarse otra vez.
    let fiscal_nuevo = if f.tipo == "global" {
        "no_requerido"
    } else {
        "requerida"
    };
    if f.tipo == "global" || f.tipo == "individual" {
        for folio in f.ventas.split(',').filter(|s| !s.trim().is_empty()) {
            conn.execute(
                "UPDATE sales SET fiscal_status = ?1 WHERE folio = ?2",
                params![fiscal_nuevo, folio.trim()],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    leer_factura(conn, id)
}

fn dejar_cancelacion(config: &ConfigFiscalDto, uuid: &str, xml: &str) -> Result<(), String> {
    let dir = Path::new(config.carpeta_salida.trim());
    fs::create_dir_all(dir).map_err(|e| format!("No pude crear la carpeta de cancelación: {e}"))?;
    let nombre = format!("CANCEL-{}", uuid.to_uppercase());
    let ruta = dir.join(format!("{nombre}.xml"));
    fs::write(&ruta, xml).map_err(|e| format!("No pude dejar {}: {e}", ruta.display()))?;
    Ok(())
}

/// Reintenta el timbrado de una factura pendiente.
///
/// Si el PAC devuelve "todavía no", la factura se queda esperando y **no** se
/// cuenta como error: reintentar no cuesta nada. Si la rechaza, sí se marca,
/// porque reintentar un comprobante mal hecho sólo gasta tiempo.
pub fn reintentar(conn: &Connection, id: i64) -> Result<FacturaDto, String> {
    let f = leer_factura(conn, id)?;
    if f.estado == "timbrada" {
        return Ok(f);
    }
    let xml: String = conn
        .query_row("SELECT xml FROM facturas WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let config = leer_config(conn);

    // Sin carpetas configuradas no se puede timbrar: mejor decirlo claro que
    // inventar un timbre falso que parezca real. Una URL HTTP se rechaza:
    // este programa no habla con ningún PAC por internet.
    let resultado = if let Some(url) = url_http_en(&config) {
        PacHttp::new(url).timbrar(&xml)
    } else if config.carpeta_salida.trim().is_empty() || config.carpeta_entrada.trim().is_empty() {
        Err(pos_fiscal::pac::ErrorPac::SinRespuesta(
            "no hay carpetas del PAC configuradas".to_string(),
        ))
    } else {
        PacCarpeta::new(&config.carpeta_salida, &config.carpeta_entrada).timbrar(&xml)
    };

    match resultado {
        Ok(t) => {
            conn.execute(
                "UPDATE facturas SET estado = 'timbrada', uuid = ?1, xml_timbrado = ?2,
                        timbrada_at = datetime('now'), ultimo_error = NULL, intentos = intentos + 1
                 WHERE id = ?3",
                params![t.uuid, t.xml, id],
            )
            .map_err(|e| e.to_string())?;
            // Con el UUID en la mano, las ventas ya están facturadas.
            for folio in f.ventas.split(',').filter(|s| !s.trim().is_empty()) {
                conn.execute(
                    "UPDATE sales SET fiscal_status = 'timbrada' WHERE folio = ?1",
                    params![folio.trim()],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Err(e) => {
            let rechazada = matches!(e, pos_fiscal::pac::ErrorPac::Rechazado(_));
            conn.execute(
                "UPDATE facturas SET intentos = intentos + 1, ultimo_error = ?1,
                        estado = ?2
                 WHERE id = ?3",
                params![
                    e.to_string(),
                    if rechazada { "error" } else { "pendiente" },
                    id
                ],
            )
            .map_err(|e| e.to_string())?;
            return Err(e.to_string());
        }
    }
    leer_factura(conn, id)
}

/// Cuántas facturas están esperando timbre. Un número que la tienda tiene que
/// poder ver sin entrar a Configuración.
pub fn pendientes(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM facturas WHERE estado = 'pendiente'",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// El PAC que toca según la configuración. Se expone para las pruebas y para
/// que la interfaz pueda decir con quién está hablando.
pub fn pac_de(config: &ConfigFiscalDto) -> Box<dyn Pac> {
    if let Some(url) = url_http_en(config) {
        Box::new(PacHttp::new(url))
    } else if config.carpeta_salida.trim().is_empty() || config.carpeta_entrada.trim().is_empty() {
        Box::new(PacDePrueba)
    } else {
        Box::new(PacCarpeta::new(
            &config.carpeta_salida,
            &config.carpeta_entrada,
        ))
    }
}

/// Deja el XML en la carpeta de salida. Si alguien puso una URL, no se escribe
/// nada: el cliente HTTP está bloqueado.
fn dejar_xml(config: &ConfigFiscalDto, xml: &str) {
    if let Some(url) = url_http_en(config) {
        eprintln!("aviso: {}", mensaje_http_bloqueado(&url));
        return;
    }
    if config.carpeta_salida.trim().is_empty() {
        return;
    }
    let pac = PacCarpeta::new(&config.carpeta_salida, &config.carpeta_entrada);
    if let Err(e) = pac.dejar(xml) {
        eprintln!("aviso: no pude dejar el XML en la carpeta: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pos_db::Database;

    fn db() -> Database {
        // `open_in_memory` no corre las migraciones (sólo configura la base),
        // así que aquí se aplican a mano.
        let d = Database::open_in_memory().unwrap();
        pos_db::migrations::migrate(d.conn()).unwrap();
        d
    }

    fn venta(conn: &Connection, folio: &str, cuando: &str, lineas: &[(i64, i64, i64)]) -> i64 {
        // lineas: (unit_price_cents, qty_millis, tax_bp)
        let total: i64 = lineas.iter().map(|(p, m, _)| p * m / 1000).sum();
        conn.execute(
            "INSERT INTO sales (folio, subtotal_cents, discount_cents, tax_cents, total_cents,
                                status, fiscal_status, created_at)
             VALUES (?1, ?2, 0, 0, ?2, 'completada', 'no_requerido', ?3)",
            params![folio, total, cuando],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        for (i, (precio, millis, bp)) in lineas.iter().enumerate() {
            conn.execute(
                "INSERT INTO sale_items (sale_id, sku, description, qty_millis,
                                         unit_price_cents, discount_cents, tax_bp, tax_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, 0)",
                params![
                    id,
                    format!("SKU{i}"),
                    format!("Producto {i}"),
                    millis,
                    precio,
                    bp
                ],
            )
            .unwrap();
        }
        id
    }

    fn config_valida() -> ConfigFiscalDto {
        ConfigFiscalDto {
            rfc: "XAXX010101000".into(),
            nombre: "Abarrotes Lupita".into(),
            regimen_fiscal: "626".into(),
            codigo_postal: "91260".into(),
            forma_pago: "01".into(),
            periodicidad: "01".into(),
            carpeta_salida: String::new(),
            carpeta_entrada: String::new(),
        }
    }

    #[test]
    fn la_global_cuadra_con_lo_que_se_cobró_ese_dia() {
        let d = db();
        let c = d.conn();
        guardar_config(c, &config_valida()).unwrap();
        venta(
            c,
            "A-1",
            "2026-10-11 10:00:00",
            &[(1800, 1000, 1600), (2320, 1000, 1600)],
        );
        venta(c, "A-2", "2026-10-11 19:30:00", &[(5000, 1000, 1600)]);
        // Esta es de otro día: no debe entrar.
        venta(c, "A-3", "2026-10-12 09:00:00", &[(9999, 1000, 1600)]);

        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        // 18.00 + 23.20 + 50.00 = 91.20
        assert_eq!(f.total_cents, 9120, "la factura no cuadra con lo cobrado");
        assert_eq!(f.estado, "pendiente");
        assert_eq!(f.receptor_rfc, "XAXX010101000");
        assert!(
            f.ventas.contains("A-1") && f.ventas.contains("A-2"),
            "{}",
            f.ventas
        );
        assert!(!f.ventas.contains("A-3"), "metió una venta de otro día");
        assert_eq!(pendientes(c), 1);
    }

    #[test]
    fn no_se_facturan_las_ventas_de_otro_dia_ni_dos_veces() {
        let d = db();
        let c = d.conn();
        guardar_config(c, &config_valida()).unwrap();
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1800, 1000, 1600)]);

        factura_global(c, "2026-10-11", "01", None).unwrap();
        // La segunda vez tiene que decir que ya se facturó, en sus palabras.
        let e = factura_global(c, "2026-10-11", "01", None).unwrap_err();
        assert!(e.contains("ya se facturó"), "{e}");

        // Y un día sin ventas tampoco fabrica una factura vacía.
        let e = factura_global(c, "2026-10-13", "01", None).unwrap_err();
        assert!(e.contains("No hay ventas"), "{e}");
    }

    #[test]
    fn sin_datos_fiscales_no_se_fabrica_una_factura_chueca() {
        let d = db();
        let c = d.conn();
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1800, 1000, 1600)]);
        let e = factura_global(c, "2026-10-11", "01", None).unwrap_err();
        assert!(e.contains("RFC"), "debe decir qué falta: {e}");

        let mut mala = config_valida();
        mala.rfc = "XAXX010101".into(); // 10 caracteres
        assert!(revisar_config(&mala).is_err());
        mala = config_valida();
        mala.codigo_postal = "9126".into(); // 4 dígitos
        assert!(revisar_config(&mala).is_err());
        mala = config_valida();
        mala.regimen_fiscal = "abc".into();
        assert!(revisar_config(&mala).is_err());
        assert!(revisar_config(&config_valida()).is_ok());
    }

    #[test]
    fn el_timbrado_por_carpetas_cierra_el_ciclo_completo() {
        let dir = std::env::temp_dir().join(format!("pos-fiscal-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let salida = dir.join("por-timbrar");
        let entrada = dir.join("timbrados");
        std::fs::create_dir_all(&entrada).unwrap();

        let d = db();
        let c = d.conn();
        let mut cfg = config_valida();
        cfg.carpeta_salida = salida.to_string_lossy().to_string();
        cfg.carpeta_entrada = entrada.to_string_lossy().to_string();
        guardar_config(c, &cfg).unwrap();
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1800, 1000, 1600)]);

        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        // El XML quedó en la carpeta de salida, esperando al PAC.
        let dejados: Vec<_> = std::fs::read_dir(&salida)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(dejados.len(), 1, "no dejó el XML por timbrar");
        assert!(std::fs::read_to_string(dejados[0].path())
            .unwrap()
            .contains("Periodicidad=\"01\""));

        // Sin respuesta del PAC: sigue pendiente, no marcada como error.
        let e = reintentar(c, f.id).unwrap_err();
        assert!(e.contains("todavía no aparece"), "{e}");
        assert_eq!(leer_factura(c, f.id).unwrap().estado, "pendiente");
        assert_eq!(leer_factura(c, f.id).unwrap().intentos, 1);

        // El PAC (o el contador) pone el XML timbrado en la carpeta de entrada.
        let nombre = dejados[0].file_name().to_string_lossy().to_string();
        let timbrado = "<cfdi:Comprobante><cfdi:Complemento>\
             <tfd:TimbreFiscalDigital UUID=\"AAAABBBB-1111-2222-3333-444455556666\" \
             FechaTimbrado=\"2026-10-12T09:00:00\" NoCertificadoSAT=\"00001000000500000000\" \
             SelloSAT=\"sello\"/>\
             </cfdi:Complemento></cfdi:Comprobante>";
        std::fs::write(entrada.join(&nombre), timbrado).unwrap();

        let f = reintentar(c, f.id).unwrap();
        assert_eq!(f.estado, "timbrada");
        assert_eq!(
            f.uuid.as_deref(),
            Some("AAAABBBB-1111-2222-3333-444455556666")
        );
        assert_eq!(pendientes(c), 0);
        // Y las ventas quedan marcadas: nadie las va a volver a facturar.
        let estado: String = c
            .query_row(
                "SELECT fiscal_status FROM sales WHERE folio = 'A-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(estado, "timbrada");
        // El XML que se entrega es el timbrado, no el que se generó.
        assert!(xml_de(c, f.id).unwrap().contains("TimbreFiscalDigital"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn la_configuracion_se_guarda_y_se_vuelve_a_leer() {
        let d = db();
        let c = d.conn();
        let cfg = config_valida();
        guardar_config(c, &cfg).unwrap();
        assert_eq!(leer_config(c), cfg);
        // Los datos fiscales se guardan en mayúsculas al armar el comprobante,
        // no al guardarlos: lo que escribió la persona se respeta tal cual.
        let mut otra = cfg.clone();
        otra.nombre = "Mi Tiendita".into();
        guardar_config(c, &otra).unwrap();
        assert_eq!(leer_config(c).nombre, "Mi Tiendita");
    }

    #[test]
    fn una_url_http_no_se_guarda_como_carpeta() {
        let d = db();
        let mut cfg = config_valida();
        cfg.carpeta_salida = "https://api.facturapi.io/v2/invoices".into();
        cfg.carpeta_entrada = "/tmp/timbrados".into();
        let e = guardar_config(d.conn(), &cfg).unwrap_err();
        assert!(e.contains("bloqueado"), "{e}");
        assert!(leer_config(d.conn()).carpeta_salida.is_empty());
        assert!(revisar_config(&cfg).is_err());
        assert_eq!(
            url_http_en(&cfg).as_deref(),
            Some("https://api.facturapi.io/v2/invoices")
        );
    }

    #[test]
    fn reintentar_por_http_se_rechaza_sin_inventar_timbre() {
        let d = db();
        let c = d.conn();
        // Se cuela una URL directo en settings (como si alguien tocara la base).
        c.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('fiscal_carpeta_salida', ?1)",
            params!["https://pac.ejemplo.mx/timbrar"],
        )
        .unwrap();
        c.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('fiscal_carpeta_entrada', ?1)",
            params!["https://pac.ejemplo.mx/timbrados"],
        )
        .unwrap();
        c.execute(
            "INSERT INTO facturas (folio, tipo, periodo, periodicidad, receptor_rfc,
                subtotal_cents, iva_cents, total_cents, estado, xml, ventas)
             VALUES ('G-1', 'individual', '2026-10', '01', 'XAXX010101000', 100, 16, 116, 'pendiente',
                     '<cfdi:Comprobante/>', '')",
            [],
        )
        .unwrap();
        let id = c.last_insert_rowid();
        let e = reintentar(c, id).unwrap_err();
        assert!(e.contains("bloqueado"), "{e}");
        assert_eq!(leer_factura(c, id).unwrap().estado, "error");
        assert!(leer_factura(c, id).unwrap().uuid.is_none());
        assert_eq!(pac_de(&leer_config(c)).nombre(), "HTTP bloqueado");
    }

    /// Timbre de mentira: pone el XML sellado donde el programa lo busca.
    fn simular_timbre(entrada: &std::path::Path, nombre: &str, uuid: &str) {
        std::fs::write(
            entrada.join(format!("{nombre}.xml")),
            format!(
                "<cfdi:Comprobante><cfdi:Complemento>\
                 <tfd:TimbreFiscalDigital UUID=\"{uuid}\" FechaTimbrado=\"2026-10-12T09:00:00\" \
                 NoCertificadoSAT=\"00001000000500000000\" SelloSAT=\"s\"/>\
                 </cfdi:Complemento></cfdi:Comprobante>"
            ),
        )
        .unwrap();
    }

    #[test]
    fn la_nota_de_credito_se_liga_a_la_factura_que_la_origino() {
        let dir = std::env::temp_dir().join(format!("pos-nc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let salida = dir.join("por-timbrar");
        let entrada = dir.join("timbrados");
        std::fs::create_dir_all(&entrada).unwrap();

        let d = db();
        let c = d.conn();
        let mut cfg = config_valida();
        cfg.carpeta_salida = salida.to_string_lossy().to_string();
        cfg.carpeta_entrada = entrada.to_string_lossy().to_string();
        guardar_config(c, &cfg).unwrap();
        venta(
            c,
            "A-1",
            "2026-10-11 10:00:00",
            &[(1800, 1000, 1600), (2320, 1000, 1600)],
        );
        venta(c, "A-2", "2026-10-11 11:00:00", &[(5000, 1000, 1600)]);

        // Sin facturar: no hay de qué restar.
        let e = nota_credito(c, "A-1", "2026-10-12", &[], None).unwrap_err();
        assert!(e.contains("todavía no está facturada"), "{e}");

        // Se factura el día y se timbra (con el PAC de carpetas).
        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        let nombre: String = std::fs::read_dir(&salida)
            .unwrap()
            .filter_map(|e| e.ok())
            .next()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".xml")
            .to_string();
        simular_timbre(&entrada, &nombre, "AAAABBBB-1111-2222-3333-444455556666");
        let f = reintentar(c, f.id).unwrap();
        assert_eq!(f.estado, "timbrada");

        // Ahora sí: la nota trae el UUID de esa factura.
        let nc = nota_credito(c, "A-1", "2026-10-12", &[], None).unwrap();
        assert_eq!(nc.tipo, "egreso");
        assert_eq!(
            nc.total_cents,
            1800 + 2320,
            "la nota dice lo que se devolvió"
        );
        let xml = xml_de(c, nc.id).unwrap();
        assert!(xml.contains("TipoDeComprobante=\"E\""), "{xml}");
        assert!(
            xml.contains("UUID=\"AAAABBBB-1111-2222-3333-444455556666\""),
            "sin el vínculo la nota no vale: {xml}"
        );
        assert!(
            !xml.contains("FormaPago"),
            "un egreso no lleva forma de pago: {xml}"
        );

        // La venta ya se devolvió completa: no queda nada que abonar.
        let e = nota_credito(c, "A-1", "2026-10-12", &[], None).unwrap_err();
        assert!(e.contains("ya está devuelta"), "{e}");

        // La otra venta sí puede tener la suya.
        let nc2 = nota_credito(c, "A-2", "2026-10-12", &[], None).unwrap();
        assert_eq!(nc2.total_cents, 5000);
        assert_ne!(nc.id, nc2.id);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Cinco cocas, devuelven dos: la nota dice dos, no cinco.
    ///
    /// Éste es el caso de todos los días en una tienda, y por eso existe la
    /// nota parcial: antes había que devolver la venta completa o nada.
    #[test]
    fn la_nota_parcial_solo_cubre_lo_que_se_devuelve() {
        let dir = std::env::temp_dir().join(format!("pos-ncp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let salida = dir.join("por-timbrar");
        let entrada = dir.join("timbrados");
        std::fs::create_dir_all(&entrada).unwrap();

        let d = db();
        let c = d.conn();
        let mut cfg = config_valida();
        cfg.carpeta_salida = salida.to_string_lossy().to_string();
        cfg.carpeta_entrada = entrada.to_string_lossy().to_string();
        guardar_config(c, &cfg).unwrap();
        // 5 cocas de $18.00 y un pan de $23.20
        venta(
            c,
            "A-1",
            "2026-10-11 10:00:00",
            &[(1800, 5000, 1600), (2320, 1000, 1600)],
        );

        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        let nombre: String = std::fs::read_dir(&salida)
            .unwrap()
            .filter_map(|e| e.ok())
            .next()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".xml")
            .to_string();
        simular_timbre(&entrada, &nombre, "1111-2222-3333");
        assert_eq!(reintentar(c, f.id).unwrap().estado, "timbrada");

        // Devuelven dos cocas: 2 × $18.00 = $36.00
        let nc = nota_credito(c, "A-1", "2026-10-12", &[("SKU0".into(), 2000)], None).unwrap();
        assert_eq!(nc.total_cents, 3600, "la nota cubre dos cocas, no cinco");
        assert_eq!(nc.tipo, "egreso");
        let xml = xml_de(c, nc.id).unwrap();
        assert!(
            xml.contains("2 x Producto 0"),
            "dice qué se devolvió: {xml}"
        );

        // Quedan tres. Se devuelven en una segunda nota.
        let nc2 = nota_credito(c, "A-1", "2026-10-12", &[("SKU0".into(), 3000)], None).unwrap();
        assert_eq!(nc2.total_cents, 5400);
        assert_ne!(nc.id, nc2.id, "son dos notas distintas");

        // Ahora las cocas ya están completas: no se puede devolver ni una más.
        let e = nota_credito(c, "A-1", "2026-10-12", &[("SKU0".into(), 1000)], None).unwrap_err();
        assert!(e.contains("Sólo quedan 0"), "{e}");

        // El pan sigue entero.
        let nc3 = nota_credito(c, "A-1", "2026-10-12", &[("SKU1".into(), 1000)], None).unwrap();
        assert_eq!(nc3.total_cents, 2320);

        // Y ya no queda nada de la venta.
        let e = nota_credito(c, "A-1", "2026-10-12", &[], None).unwrap_err();
        assert!(e.contains("ya está devuelta"), "{e}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Devolver más de lo que se vendió no es una devolución: es regalar
    /// dinero con una factura encima.
    #[test]
    fn nadie_devuelve_mas_de_lo_que_compro() {
        let d = db();
        let c = d.conn();
        let mut cfg = config_valida();
        cfg.carpeta_salida = String::new();
        cfg.carpeta_entrada = String::new();
        guardar_config(c, &cfg).unwrap();
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1800, 2000, 1600)]);
        factura_global(c, "2026-10-11", "01", None).unwrap();
        // Sin timbrar no hay nota... así que la probamos donde sí aplica:
        let e = nota_credito(c, "A-1", "2026-10-12", &[("SKU0".into(), 9000)], None).unwrap_err();
        assert!(
            e.contains("no está facturada")
                || e.contains("Sólo quedan")
                || e.contains("No encontré"),
            "{e}"
        );
    }

    /// Un SKU que la venta no vendió no se puede devolver.
    #[test]
    fn no_se_devuelve_lo_que_no_se_vendio() {
        let dir = std::env::temp_dir().join(format!("pos-ncs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let entrada = dir.join("timbrados");
        std::fs::create_dir_all(&entrada).unwrap();
        let salida = dir.join("por-timbrar");

        let d = db();
        let c = d.conn();
        let mut cfg = config_valida();
        cfg.carpeta_salida = salida.to_string_lossy().to_string();
        cfg.carpeta_entrada = entrada.to_string_lossy().to_string();
        guardar_config(c, &cfg).unwrap();
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1800, 2000, 1600)]);
        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        let nombre: String = std::fs::read_dir(&salida)
            .unwrap()
            .filter_map(|e| e.ok())
            .next()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".xml")
            .to_string();
        simular_timbre(&entrada, &nombre, "ABC");
        reintentar(c, f.id).unwrap();

        let e = nota_credito(c, "A-1", "2026-10-12", &[("9999".into(), 1000)], None).unwrap_err();
        assert!(e.contains("no vendió 9999"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Un cliente con RFC: su venta espera SU factura, no la del día.
    ///
    /// Si se metiera en la global, el SAT vería el mismo ingreso dos veces y
    /// el contador lo descubriría hasta marzo.
    #[test]
    fn una_venta_de_cliente_con_rfc_no_se_mete_en_la_global() {
        use pos_db::clientes::{Clientes, NuevoCliente};
        use pos_db::products::NewProduct as ProductoNuevo;

        let mut db = pos_db::Database::open_in_memory().unwrap();
        guardar_config(db.conn(), &config_valida()).unwrap();

        let cliente = Clientes::crear(
            db.conn_mut(),
            &NuevoCliente {
                name: "Restaurante El Sol".into(),
                rfc: "RES123456T1A".into(),
                phone: String::new(),
                email: String::new(),
                credit_limit_cents: 0,
                regimen_fiscal: "601".into(),
                codigo_postal: "91260".into(),
                uso_cfdi: "G03".into(),
            },
        )
        .unwrap();
        let pid = pos_db::products::Products::insert(
            db.conn_mut(),
            &ProductoNuevo {
                sku: "7501".into(),
                name: "Coca-Cola 600 ml".into(),
                price_cents: 1800,
                cost_cents: 900,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: 1_000_000,
                min_stock_millis: 0,
                track_inventory: true,
                barcodes: vec![],
            },
        )
        .unwrap();

        let mut v = pos_core::Sale::new("");
        v.customer_id = Some(cliente);
        v.add_line(pos_core::SaleLine {
            product_id: Some(pid),
            sku: "7501".into(),
            description: "Coca-Cola 600 ml".into(),
            qty: pos_core::Quantity::from_units(5),
            unit_price: pos_core::Money::from_cents(1800),
            discount: pos_core::Money::ZERO,
            tax: pos_core::TaxRate::IVA_16,
            unit_cost: pos_core::Money::from_cents(900),
        })
        .unwrap();
        v.add_payment(pos_core::Payment::new(
            pos_core::PaymentMethod::Efectivo,
            pos_core::Money::from_cents(9000),
        ));
        pos_db::sales::Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();
        // `save` devuelve el id; el folio con el que se factura es el de la venta.
        let folio = v.folio.clone();

        let c = db.conn();
        // Quedó marcada: espera factura individual.
        let estado: String = c
            .query_row(
                "SELECT fiscal_status FROM sales WHERE folio = ?1",
                params![&folio],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(estado, "requerida", "la venta con RFC espera su factura");
        assert_eq!(ventas_por_facturar(c).unwrap(), 1);

        // Así que la global del día no la toca.
        let e = factura_global(c, "2026-10-11", "01", None).unwrap_err();
        assert!(
            e.to_lowercase().contains("no hay") || e.to_lowercase().contains("ninguna"),
            "la global no se come la venta con RFC: {e}"
        );

        // Y la factura individual sí sale, a nombre del cliente.
        let f = facturar_venta(c, &folio, "2026-10-11", None).unwrap();
        assert_eq!(f.tipo, "individual");
        assert_eq!(f.receptor_rfc, "RES123456T1A");
        assert_eq!(f.total_cents, 9000);
        let xml = xml_de(c, f.id).unwrap();
        assert!(xml.contains("Rfc=\"RES123456T1A\""), "{xml}");
        assert!(xml.contains("RegimenFiscalReceptor=\"601\""), "{xml}");
        assert!(xml.contains("DomicilioFiscalReceptor=\"91260\""), "{xml}");
        assert!(xml.contains("UsoCFDI=\"G03\""), "{xml}");
        assert!(xml.contains("TipoDeComprobante=\"I\""), "{xml}");

        // Ya no está pendiente, y no se factura dos veces.
        assert_eq!(ventas_por_facturar(c).unwrap(), 0);
        let e = facturar_venta(c, &folio, "2026-10-11", None).unwrap_err();
        assert!(e.contains("ya está facturada"), "{e}");
    }

    /// Al cliente que le faltan datos no se le factura, y se le dice qué es.
    #[test]
    fn sin_datos_fiscales_completos_no_se_factura() {
        use pos_db::clientes::{Clientes, NuevoCliente};

        let mut db = pos_db::Database::open_in_memory().unwrap();
        guardar_config(db.conn(), &config_valida()).unwrap();

        // RFC sí, régimen y código postal no: le falta para facturar.
        let cliente = Clientes::crear(
            db.conn_mut(),
            &NuevoCliente {
                name: "Don Chuy".into(),
                rfc: "MAGM800101QX3".into(),
                phone: String::new(),
                email: String::new(),
                credit_limit_cents: 0,
                regimen_fiscal: String::new(),
                codigo_postal: String::new(),
                uso_cfdi: String::new(),
            },
        )
        .unwrap();
        assert!(
            !pos_db::clientes::datos_fiscales_completos(db.conn(), cliente),
            "sin régimen ni código postal no se le puede facturar"
        );
    }

    /// Una factura a crédito se va saldando con complementos, de a poco.
    ///
    /// Éste es el caso del restaurante que compra hoy y paga la otra semana,
    /// o que liquida en dos exhibiciones. La factura original dice "PPD" y se
    /// queda debiendo; cada pago va en su propio comprobante tipo P ligado
    /// por UUID al documento que está cubriendo.
    #[test]
    fn el_complemento_sigue_la_pista_de_cuanto_falta() {
        use pos_db::clientes::{Clientes, NuevoCliente};
        use pos_db::products::NewProduct as ProductoNuevo;

        let dir = std::env::temp_dir().join(format!("pos-cp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let salida = dir.join("por-timbrar");
        let entrada = dir.join("timbrados");
        std::fs::create_dir_all(&entrada).unwrap();

        let mut db = pos_db::Database::open_in_memory().unwrap();
        let mut cfg = config_valida();
        cfg.carpeta_salida = salida.to_string_lossy().to_string();
        cfg.carpeta_entrada = entrada.to_string_lossy().to_string();
        guardar_config(db.conn(), &cfg).unwrap();

        let cliente = Clientes::crear(
            db.conn_mut(),
            &NuevoCliente {
                name: "Restaurante El Sol".into(),
                rfc: "RES123456T1A".into(),
                phone: String::new(),
                email: String::new(),
                credit_limit_cents: 500_000,
                regimen_fiscal: "601".into(),
                codigo_postal: "91260".into(),
                uso_cfdi: "G03".into(),
            },
        )
        .unwrap();
        let pid = pos_db::products::Products::insert(
            db.conn_mut(),
            &ProductoNuevo {
                sku: "7501".into(),
                name: "Costal de arroz".into(),
                price_cents: 500_000,
                cost_cents: 300_000,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: 1_000_000,
                min_stock_millis: 0,
                track_inventory: true,
                barcodes: vec![],
            },
        )
        .unwrap();

        // Se lleva $5,000.00 a crédito.
        let mut v = pos_core::Sale::new("");
        v.customer_id = Some(cliente);
        v.add_line(pos_core::SaleLine {
            product_id: Some(pid),
            sku: "7501".into(),
            description: "Costal de arroz".into(),
            qty: pos_core::Quantity::from_units(1),
            unit_price: pos_core::Money::from_cents(500_000),
            discount: pos_core::Money::ZERO,
            tax: pos_core::TaxRate::IVA_16,
            unit_cost: pos_core::Money::from_cents(300_000),
        })
        .unwrap();
        v.add_payment(pos_core::Payment::new(
            pos_core::PaymentMethod::Credito,
            pos_core::Money::from_cents(500_000),
        ));
        pos_db::sales::Sales::save(db.conn_mut(), &mut v, 1, None).unwrap();
        let folio = v.folio.clone();

        let c = db.conn();
        let f = facturar_venta(c, &folio, "2026-10-11", None).unwrap();
        assert_eq!(f.tipo, "individual");
        let xml = xml_de(c, f.id).unwrap();
        assert!(
            xml.contains(r#"MetodoPago="PPD""#),
            "la factura a crédito es PPD: {xml}"
        );
        // Quedó debiendo todo.
        assert_eq!(saldo_por_pagar(c, cliente).unwrap(), 500_000);

        // Se timbra: sin UUID no hay manera de ligarle un complemento.
        let nombre: String = std::fs::read_dir(&salida)
            .unwrap()
            .filter_map(|e| e.ok())
            .next()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".xml")
            .to_string();
        simular_timbre(&entrada, &nombre, "FACTURA-CREDITO-1");
        assert_eq!(reintentar(c, f.id).unwrap().estado, "timbrada");

        // Paga la mitad: $2,500.00 en transferencia.
        let cp = complemento_pago(c, cliente, 250_000, "2026-10-20", "03", None).unwrap();
        assert_eq!(cp.tipo, "pago");
        let xml = xml_de(c, cp.id).unwrap();
        assert!(xml.contains(r#"TipoDeComprobante="P""#), "{xml}");
        assert!(
            xml.contains(r#"IdDocumento="FACTURA-CREDITO-1""#),
            "sin el UUID de la factura el complemento no vale: {xml}"
        );
        assert!(xml.contains(r#"NumParcialidad="1""#), "{xml}");
        assert!(xml.contains(r#"FormaDePagoP="03""#), "{xml}");
        assert!(xml.contains(r#"Monto="2500.00""#), "{xml}");
        assert!(
            xml.contains(r#"Folio="#) && cp.folio.starts_with("CP-"),
            "el complemento trae su propio folio: {xml}"
        );
        // Con impuestos, el documento va marcado "02" y el pago trae su
        // resumen: sin eso el PAC lo rechaza.
        assert!(xml.contains(r#"ObjetoImpDR="02""#), "{xml}");
        assert!(xml.contains(r#"ImporteP="344.83""#), "{xml}");
        assert!(xml.contains(r#"BaseP="2155.17""#), "{xml}");
        // Lo que pagó y lo que sigue debiendo: $2,500.00 y le faltan $2,500.00.
        assert!(xml.contains(r#"ImpPagado="2500.00""#), "{xml}");
        assert!(
            xml.contains(r#"ImpSaldoInsoluto="2500.00""#),
            "le faltan $2,500: {xml}"
        );

        // El saldo bajó a la mitad.
        assert_eq!(saldo_por_pagar(c, cliente).unwrap(), 250_000);

        // Paga el resto. Ahora es la segunda parcialidad de esa factura.
        let cp2 = complemento_pago(c, cliente, 250_000, "2026-10-27", "01", None).unwrap();
        let xml2 = xml_de(c, cp2.id).unwrap();
        assert!(
            xml2.contains(r#"NumParcialidad="2""#),
            "el segundo pago es la parcialidad 2: {xml2}"
        );
        assert!(
            xml2.contains(r#"ImpSaldoAnt="2500.00""#),
            "antes debía $2,500: {xml2}"
        );
        assert!(
            xml2.contains(r#"ImpSaldoInsoluto="0.00""#),
            "ya quedó saldada: {xml2}"
        );
        assert_eq!(saldo_por_pagar(c, cliente).unwrap(), 0);

        // Saldada: ya no acepta más pagos, y lo dice claro.
        let e = complemento_pago(c, cliente, 10_000, "2026-10-28", "01", None).unwrap_err();
        assert!(e.contains("no tiene facturas a crédito pendientes"), "{e}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Pagar más de lo que se debe no es un pago: es prestar dinero al cliente
    /// con una factura encima.
    #[test]
    fn nadie_paga_mas_de_lo_que_debe() {
        use pos_db::clientes::{Clientes, NuevoCliente};

        let mut db = pos_db::Database::open_in_memory().unwrap();
        guardar_config(db.conn(), &config_valida()).unwrap();
        let cliente = Clientes::crear(
            db.conn_mut(),
            &NuevoCliente {
                name: "Don Chuy".into(),
                rfc: "MAGM800101QX3".into(),
                phone: String::new(),
                email: String::new(),
                credit_limit_cents: 50_000,
                regimen_fiscal: "601".into(),
                codigo_postal: "91260".into(),
                uso_cfdi: "G03".into(),
            },
        )
        .unwrap();

        let c = db.conn();
        // Sin facturas a crédito no hay complemento que emitir.
        let e = complemento_pago(c, cliente, 10_000, "2026-10-20", "01", None).unwrap_err();
        assert!(e.contains("no tiene facturas a crédito pendientes"), "{e}");

        // Y un monto de cero tampoco es un pago.
        let e = complemento_pago(c, cliente, 0, "2026-10-20", "01", None).unwrap_err();
        assert!(e.contains("mayor a cero"), "{e}");
    }

    /// A quien no tiene RFC no se le emite un complemento.
    #[test]
    fn sin_rfc_no_hay_complemento_de_pagos() {
        use pos_db::clientes::{Clientes, NuevoCliente};

        let mut db = pos_db::Database::open_in_memory().unwrap();
        guardar_config(db.conn(), &config_valida()).unwrap();
        let cliente = Clientes::crear(
            db.conn_mut(),
            &NuevoCliente {
                name: "Público en general".into(),
                rfc: pos_fiscal::cfdi::RFC_PUBLICO.into(),
                phone: String::new(),
                email: String::new(),
                credit_limit_cents: 0,
                regimen_fiscal: String::new(),
                codigo_postal: String::new(),
                uso_cfdi: String::new(),
            },
        )
        .unwrap();
        let e = complemento_pago(db.conn(), cliente, 10_000, "2026-10-20", "01", None).unwrap_err();
        assert!(e.contains("no tiene RFC"), "{e}");
    }

    /// "Te devuelvo $36" en vez de "te devuelvo dos cocas".
    #[test]
    fn la_nota_por_monto_devuelve_el_dinero_pedido() {
        let dir = std::env::temp_dir().join(format!("pos-ncm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let entrada = dir.join("timbrados");
        let salida = dir.join("por-timbrar");
        std::fs::create_dir_all(&entrada).unwrap();

        let d = db();
        let c = d.conn();
        let mut cfg = config_valida();
        cfg.carpeta_salida = salida.to_string_lossy().to_string();
        cfg.carpeta_entrada = entrada.to_string_lossy().to_string();
        guardar_config(c, &cfg).unwrap();
        // Cinco cosas de $18.00 = $90.00
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1800, 5000, 1600)]);

        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        let nombre: String = std::fs::read_dir(&salida)
            .unwrap()
            .filter_map(|e| e.ok())
            .next()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".xml")
            .to_string();
        simular_timbre(&entrada, &nombre, "MONTO-1");
        assert_eq!(reintentar(c, f.id).unwrap().estado, "timbrada");

        // $36.00 son dos piezas exactas.
        let (nc, _) = nota_credito_monto(c, "A-1", "2026-10-12", 3600, None).unwrap();
        assert_eq!(nc.total_cents, 3600, "dos piezas de $18");
        assert_eq!(nc.tipo, "egreso");

        // Quedan $54.00 vivos. Pedir $100 no se puede, y lo dice con número.
        let e = nota_credito_monto(c, "A-1", "2026-10-12", 10_000, None).unwrap_err();
        assert!(e.contains("no alcanza"), "{e}");

        // Y devolver el resto sí se puede.
        let (nc2, _) = nota_credito_monto(c, "A-1", "2026-10-12", 5400, None).unwrap();
        assert_eq!(nc2.total_cents, 5400);
        assert_ne!(nc.id, nc2.id);

        // Ya no queda nada.
        let e = nota_credito_monto(c, "A-1", "2026-10-12", 100, None).unwrap_err();
        assert!(
            e.contains("ya está devuelta") || e.contains("no alcanza"),
            "{e}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// El reparto entre renglones no pierde ni inventa centavos.
    #[test]
    fn el_reparto_por_monto_suma_exacto() {
        let dir = std::env::temp_dir().join(format!("pos-ncr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let entrada = dir.join("timbrados");
        let salida = dir.join("por-timbrar");
        std::fs::create_dir_all(&entrada).unwrap();

        let d = db();
        let c = d.conn();
        let mut cfg = config_valida();
        cfg.carpeta_salida = salida.to_string_lossy().to_string();
        cfg.carpeta_entrada = entrada.to_string_lossy().to_string();
        guardar_config(c, &cfg).unwrap();
        venta(
            c,
            "A-1",
            "2026-10-11 10:00:00",
            &[(1000, 3000, 1600), (700, 3000, 1600)],
        );
        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        let nombre: String = std::fs::read_dir(&salida)
            .unwrap()
            .filter_map(|e| e.ok())
            .next()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".xml")
            .to_string();
        simular_timbre(&entrada, &nombre, "REPARTO-1");
        reintentar(c, f.id).unwrap();

        // $51.00 de $51.00: se pide todo y cubre todo.
        let (nc, _) = nota_credito_monto(c, "A-1", "2026-10-12", 5100, None).unwrap();
        assert!(
            (nc.total_cents - 5100).abs() <= 2,
            "el reparto no pierde centavos: pidió 5100 y cubrió {}",
            nc.total_cents
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn una_venta_que_no_existe_no_se_presta_a_confusiones() {
        let d = db();
        let c = d.conn();
        guardar_config(c, &config_valida()).unwrap();
        let e = nota_credito(c, "A-99", "2026-10-12", &[], None).unwrap_err();
        assert!(
            e.contains("No encontré") || e.contains("no está facturada"),
            "{e}"
        );
    }

    #[test]
    fn el_bimestre_sale_con_los_dos_meses() {
        assert_eq!(
            meses_y_anio("2026-01-15", "05"),
            ("01-02".to_string(), "2026".to_string())
        );
        assert_eq!(
            meses_y_anio("2026-02-15", "05"),
            ("01-02".to_string(), "2026".to_string())
        );
        assert_eq!(
            meses_y_anio("2026-03-15", "05"),
            ("03-04".to_string(), "2026".to_string())
        );
        assert_eq!(
            meses_y_anio("2026-10-11", "01"),
            ("10".to_string(), "2026".to_string())
        );
    }

    #[test]
    fn una_tasa_desconocida_se_trata_como_exenta_y_no_inventa_iva() {
        let d = db();
        let c = d.conn();
        guardar_config(c, &config_valida()).unwrap();
        // tax_bp = 999: una tasa que el sistema no conoce.
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1000, 1000, 999)]);
        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        assert_eq!(f.iva_cents, 0, "inventó un IVA que nadie cobró");
        assert_eq!(f.total_cents, 1000);
    }

    #[test]
    fn cancelar_una_pendiente_no_pide_uuid_ni_deja_xml() {
        let d = db();
        let c = d.conn();
        guardar_config(c, &config_valida()).unwrap();
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1800, 1000, 1600)]);
        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        assert_eq!(f.estado, "pendiente");
        let c2 = cancelar(c, f.id, "02", "").unwrap();
        assert_eq!(c2.estado, "cancelada");
        let status: String = c
            .query_row(
                "SELECT fiscal_status FROM sales WHERE folio = 'A-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "no_requerido");
        // Se puede volver a emitir la global de ese día.
        let otra = factura_global(c, "2026-10-11", "01", None).unwrap();
        assert_ne!(otra.id, f.id);
    }

    #[test]
    fn cancelar_una_timbrada_deja_el_xml_con_motivo_y_uuid() {
        let dir = std::env::temp_dir().join(format!("pos-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let salida = dir.join("por-timbrar");
        let entrada = dir.join("timbrados");
        std::fs::create_dir_all(&entrada).unwrap();

        let d = db();
        let c = d.conn();
        let mut cfg = config_valida();
        cfg.carpeta_salida = salida.to_string_lossy().to_string();
        cfg.carpeta_entrada = entrada.to_string_lossy().to_string();
        guardar_config(c, &cfg).unwrap();
        venta(c, "A-1", "2026-10-11 10:00:00", &[(1800, 1000, 1600)]);
        let f = factura_global(c, "2026-10-11", "01", None).unwrap();
        let nombre: String = std::fs::read_dir(&salida)
            .unwrap()
            .filter_map(|e| e.ok())
            .next()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".xml")
            .to_string();
        simular_timbre(&entrada, &nombre, "AAAA1111-2222-3333-4444-555566667777");
        reintentar(c, f.id).unwrap();

        let c2 = cancelar(c, f.id, "02", "").unwrap();
        assert_eq!(c2.estado, "cancelada");
        let xml = xml_de(c, f.id).unwrap();
        assert!(xml.contains("<Motivo>02</Motivo>"));
        assert!(xml.contains("AAAA1111-2222-3333-4444-555566667777"));
        let archivo = salida.join("CANCEL-AAAA1111-2222-3333-4444-555566667777.xml");
        assert!(archivo.exists(), "no dejó la solicitud en la carpeta");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancelar_con_motivo_01_sin_sustitucion_se_rechaza() {
        let d = db();
        let c = d.conn();
        c.execute(
            "INSERT INTO facturas (folio, tipo, periodo, periodicidad, receptor_rfc,
                subtotal_cents, iva_cents, total_cents, estado, uuid, xml, ventas)
             VALUES ('G-1', 'individual', '2026-10-12', '', 'XAXX010101000', 100, 16, 116,
                     'timbrada', 'AAAA1111-2222-3333-4444-555566667777', '<cfdi/>', '')",
            [],
        )
        .unwrap();
        let id = c.last_insert_rowid();
        let e = cancelar(c, id, "01", "").unwrap_err();
        assert!(e.contains("sustituye"), "{e}");
        assert_eq!(leer_factura(c, id).unwrap().estado, "timbrada");
    }
}
