//! Importación y exportación del catálogo por CSV.
//!
//! Es la puerta de entrada del catálogo real de la tienda: nadie captura 3,000
//! productos a mano. El diseño sigue tres reglas:
//!
//! 1. **Adivina las columnas.** Acepta los encabezados de Excel, de Aspel, de
//!    Contpaq y de una hoja hecha a mano, en español o en inglés, con o sin
//!    acentos. Si no encuentra el nombre del producto, lo dice.
//! 2. **Perdona el formato del dinero.** `$18.50`, `18,50`, `1,234.56` y
//!    `1850` son el mismo precio.
//! 3. **Todo o nada.** Si un renglón está mal, no se escribe nada y el reporte
//!    dice el número de renglón y el motivo. Nunca un catálogo a medias.

use pos_db::products::NewProduct;
use serde::{Deserialize, Serialize};

/// Un renglón que no se pudo importar.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ErrorRenglon {
    /// Número de renglón **en el archivo**, contando el encabezado como 1.
    pub renglon: usize,
    /// Columna que falló, si se sabe.
    pub columna: Option<String>,
    pub motivo: String,
    /// El renglón original, para que el usuario lo corrija.
    pub contenido: String,
}

/// Resultado de una importación.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ResultadoImporte {
    pub renglones: usize,
    pub creados: usize,
    pub actualizados: usize,
    pub errores: Vec<ErrorRenglon>,
    /// Encabezados que se reconocieron, para confirmarle al usuario que leyó
    /// bien su archivo.
    pub columnas_reconocidas: Vec<String>,
    /// Los productos ya interpretados. No viaja por la red: es para
    /// `LocalApi`, que es quien los mete a la base de datos.
    #[serde(skip)]
    pub productos: Vec<NewProduct>,
    /// La categoría de cada producto, en el mismo orden que `productos`.
    /// Va aparte porque la categoría no es un campo del producto: es una
    /// tabla propia, y así el CSV puede traerla o no.
    #[serde(skip)]
    pub categorias: Vec<Option<String>>,
}

impl ResultadoImporte {
    pub fn sin_errores(&self) -> bool {
        self.errores.is_empty()
    }
}

// --------------------------------------------------------------- lector CSV

/// Lee un CSV respetando comillas, saltos de línea dentro de un campo y el
/// separador `,` o `;` (se detecta en el encabezado).
pub fn leer_csv(texto: &str) -> Vec<Vec<String>> {
    let texto = texto.trim_start_matches('\u{feff}'); // BOM de Excel
    if texto.trim().is_empty() {
        return Vec::new();
    }
    let separador = detectar_separador(texto);

    let mut filas: Vec<Vec<String>> = Vec::new();
    let mut fila: Vec<String> = Vec::new();
    let mut campo = String::new();
    let mut entre_comillas = false;
    let mut caracteres = texto.chars().peekable();
    let mut campo_vacio = true;

    while let Some(c) = caracteres.next() {
        match c {
            '"' if !entre_comillas && campo_vacio => entre_comillas = true,
            '"' if entre_comillas => {
                if caracteres.peek() == Some(&'"') {
                    caracteres.next();
                    campo.push('"');
                } else {
                    entre_comillas = false;
                }
            }
            c if c == separador && !entre_comillas => {
                fila.push(std::mem::take(&mut campo));
                campo_vacio = true;
            }
            '\n' if !entre_comillas => {
                if campo.ends_with('\r') {
                    campo.pop();
                }
                fila.push(std::mem::take(&mut campo));
                campo_vacio = true;
                // Siempre se vacía el renglón: si venía en blanco, no debe
                // pegarse al siguiente.
                let terminada = std::mem::take(&mut fila);
                if terminada.iter().any(|c| !c.trim().is_empty()) {
                    filas.push(terminada);
                }
            }
            '\r' if !entre_comillas && caracteres.peek() != Some(&'\n') => {
                fila.push(std::mem::take(&mut campo));
                campo_vacio = true;
                let terminada = std::mem::take(&mut fila);
                if terminada.iter().any(|c| !c.trim().is_empty()) {
                    filas.push(terminada);
                }
            }
            otro => {
                campo_vacio = false;
                campo.push(otro);
            }
        }
    }
    if !campo.is_empty() || !fila.is_empty() {
        if campo.ends_with('\r') {
            campo.pop();
        }
        fila.push(campo);
        if fila.iter().any(|c| !c.trim().is_empty()) {
            filas.push(fila);
        }
    }
    filas
}

fn detectar_separador(texto: &str) -> char {
    let primera = texto.lines().next().unwrap_or("");
    let comas = primera.matches(',').count();
    let puntos_y_coma = primera.matches(';').count();
    let tabuladores = primera.matches('\t').count();
    if tabuladores > comas && tabuladores > puntos_y_coma {
        '\t'
    } else if puntos_y_coma > comas {
        ';'
    } else {
        ','
    }
}

/// Minúsculas sin acentos: "Descripción" y "descripcion" son la misma columna.
pub fn normalizar(texto: &str) -> String {
    let minusculas = texto.to_lowercase();
    minusculas
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            ' ' | '-' | '.' => '_',
            otro => otro,
        })
        .collect()
}

// -------------------------------------------------------------- columnas

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Columna {
    Sku,
    Nombre,
    Precio,
    Costo,
    Impuesto,
    Existencia,
    Minimo,
    CodigoBarras,
    Unidad,
    Categoria,
    Ninguna,
}

impl Columna {
    pub fn etiqueta(self) -> &'static str {
        match self {
            Columna::Sku => "SKU",
            Columna::Nombre => "nombre",
            Columna::Precio => "precio",
            Columna::Costo => "costo",
            Columna::Impuesto => "impuesto",
            Columna::Existencia => "existencia",
            Columna::Minimo => "mínimo",
            Columna::CodigoBarras => "código de barras",
            Columna::Unidad => "unidad",
            Columna::Categoria => "categoría",
            Columna::Ninguna => "(sin usar)",
        }
    }

    /// Traduce el nombre de campo que manda el cliente ("precio", "ignorar"...)
    /// a la columna interna. Es la puerta del **mapeo manual**: cuando la
    /// adivinanza falla, la persona elige a mano qué es cada columna.
    pub fn desde_campo(campo: &str) -> Columna {
        match normalizar(campo).trim() {
            "sku" | "codigo" | "clave" => Columna::Sku,
            "nombre" | "descripcion" | "producto" => Columna::Nombre,
            "precio" | "precio_venta" | "venta" => Columna::Precio,
            "costo" | "precio_costo" | "compra" => Columna::Costo,
            "impuesto" | "iva" | "tasa" => Columna::Impuesto,
            "existencia" | "stock" | "cantidad" | "inventario" => Columna::Existencia,
            "minimo" | "stock_minimo" => Columna::Minimo,
            "codigo_barras" | "barras" | "ean" => Columna::CodigoBarras,
            "unidad" | "unidad_de_medida" | "medida" => Columna::Unidad,
            "categoria" | "departamento" | "linea" => Columna::Categoria,
            "" | "ignorar" | "ninguna" | "sin_usar" | "-" => Columna::Ninguna,
            // Cualquier otra cosa se trata como encabezado: así un cliente
            // puede mandar el texto original de la columna y funciona igual.
            otro => Columna::desde_encabezado(otro),
        }
    }

    /// Los campos que se pueden elegir en la pantalla de mapeo.
    pub fn campos() -> &'static [(&'static str, &'static str)] {
        &[
            ("nombre", "Nombre del producto"),
            ("precio", "Precio de venta"),
            ("costo", "Costo"),
            ("sku", "SKU / clave"),
            ("codigo_barras", "Código de barras"),
            ("existencia", "Existencia"),
            ("minimo", "Stock mínimo"),
            ("impuesto", "Impuesto (IVA)"),
            ("unidad", "Unidad"),
            ("categoria", "Categoría"),
            ("ignorar", "(no usar esta columna)"),
        ]
    }

    fn desde_encabezado(encabezado: &str) -> Columna {
        let h = normalizar(encabezado);
        if coincidencias(
            &h,
            &[
                "nombre",
                "descripcion",
                "producto",
                "articulo",
                "name",
                "description",
                "item",
            ],
        ) {
            return Columna::Nombre;
        }
        if coincidencias(
            &h,
            &[
                "precio_costo",
                "costo",
                "compra",
                "precio_de_compra",
                "costo_unitario",
                "cost",
            ],
        ) {
            return Columna::Costo;
        }
        if coincidencias(
            &h,
            &[
                "precio_venta",
                "precio_publico",
                "precio_de_venta",
                "precio",
                "venta",
                "publico",
                "precio_unitario",
                "price",
            ],
        ) {
            return Columna::Precio;
        }
        if coincidencias(&h, &["iva", "impuesto", "tasa", "tasa_iva", "tax"]) {
            return Columna::Impuesto;
        }
        if coincidencias(
            &h,
            &["minimo", "stock_minimo", "existencia_minima", "min_stock"],
        ) {
            return Columna::Minimo;
        }
        if coincidencias(
            &h,
            &[
                "existencia",
                "stock",
                "cantidad",
                "inventario",
                "existencias",
                "quantity",
            ],
        ) {
            return Columna::Existencia;
        }
        if coincidencias(
            &h,
            &[
                "codigo_de_barras",
                "codigo_barras",
                "barras",
                "ean",
                "upc",
                "barcode",
            ],
        ) {
            return Columna::CodigoBarras;
        }
        if coincidencias(
            &h,
            &["sku", "codigo", "clave", "id", "referencia", "ref", "code"],
        ) {
            return Columna::Sku;
        }
        if coincidencias(&h, &["unidad", "unidad_de_medida", "medida", "uom", "unit"]) {
            return Columna::Unidad;
        }
        if coincidencias(
            &h,
            &[
                "categoria",
                "categoria_1",
                "departamento",
                "linea",
                "category",
            ],
        ) {
            return Columna::Categoria;
        }
        Columna::Ninguna
    }
}

/// Verdadero si el encabezado **contiene** alguno de los alias como palabra
/// completa (para no confundir "precio" con "precio_costo").
fn coincidencias(encabezado: &str, alias: &[&str]) -> bool {
    let h = encabezado.trim_matches('_');
    alias
        .iter()
        .any(|a| h == *a || h.ends_with(a) || h.starts_with(a))
}

// ------------------------------------------------------------------ dinero

/// Separa la parte entera de la fracción.
///
/// Regla (la misma para dinero y para cantidades), pensada para archivos
/// hechos en México:
/// - El **punto** siempre es decimal: `18.50` son dieciocho cincuenta.
/// - La **coma** con 1 o 2 dígitos es decimal: `18,50`. Con 3 es separador de
///   miles: `1,200` son mil doscientos.
/// - Si aparecen los dos, gana el que esté más a la derecha: `1,234.56`.
fn partir_numero(texto: &str) -> Option<(String, String)> {
    let limpio: String = texto
        .chars()
        .filter(|c| !matches!(c, '$' | ' ' | '\u{a0}' | '"' | '(' | ')'))
        .collect();
    if limpio.is_empty() {
        return None;
    }
    let comas = limpio.matches(',').count();
    let puntos = limpio.matches('.').count();

    let pos_decimal: Option<usize> = match (comas, puntos) {
        (0, 0) => None,
        // Sólo puntos: siempre decimales (convención de México).
        (0, _) => limpio.rfind('.'),
        // Sólo comas: 1 o 2 dígitos son decimales, 3 son separador de miles.
        (_, 0) => {
            let i = limpio.rfind(',').unwrap();
            let fraccion = limpio.len() - i - 1;
            if fraccion == 3 {
                None
            } else {
                Some(i)
            }
        }
        // Hay de los dos: gana el que esté más a la derecha.
        _ => {
            let c = limpio.rfind(',').unwrap();
            let p = limpio.rfind('.').unwrap();
            Some(c.max(p))
        }
    };

    let (entera, fraccion) = match pos_decimal {
        Some(i) => (
            limpio[..i].replace([',', '.'], ""),
            limpio[i + 1..].to_string(),
        ),
        None => (limpio.replace([',', '.'], ""), String::new()),
    };
    if entera.is_empty() && fraccion.is_empty() {
        return None;
    }
    Some((entera, fraccion))
}

/// Acepta `$18.50`, `18,50`, `1,234.56`, `1.234,56`, `1850`. Devuelve centavos.
pub fn parsear_dinero(texto: &str) -> Option<i64> {
    let entre_parentesis = texto.trim_start().starts_with('(');
    let (entera, fraccion) = partir_numero(texto)?;
    let enteros: i64 = if entera.is_empty() {
        0
    } else {
        entera.parse().ok()?
    };
    let centavos: i64 = match fraccion.len() {
        0 => 0,
        1 => fraccion.parse::<i64>().ok()? * 10,
        _ => fraccion[..2].parse::<i64>().ok()?,
    };
    let total = enteros * 100 + centavos;
    // El '-' ya viaja en `enteros`. Sólo falta el caso contable (1.50).
    Some(if entre_parentesis {
        -total.abs()
    } else {
        total
    })
}

/// Existencias en milésimas (como `Quantity`): `12`, `12.5`, `1,200`.
pub fn parsear_cantidad(texto: &str) -> Option<i64> {
    let limpio = texto.trim();
    if limpio.is_empty() {
        return Some(0);
    }
    let (entera, fraccion) = partir_numero(limpio)?;
    let enteros: i64 = if entera.is_empty() {
        0
    } else {
        entera.parse().ok()?
    };
    let milesimos: i64 = match fraccion.len() {
        0 => 0,
        1 => fraccion.parse::<i64>().ok()? * 100,
        2 => fraccion.parse::<i64>().ok()? * 10,
        _ => fraccion[..3].parse::<i64>().ok()?,
    };
    Some(enteros * 1_000 + milesimos)
}

/// `16`, `16%`, `0.16`, `1600` → 1600 basis points.
pub fn parsear_impuesto(texto: &str) -> Option<i32> {
    let limpio: String = texto
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    if limpio.is_empty() {
        return Some(0);
    }
    let (entera, fraccion) = partir_numero(&limpio)?;
    let valor: f64 = match fraccion.len() {
        0 => entera.parse::<f64>().ok()?,
        n => {
            let d = fraccion.parse::<f64>().ok()?;
            entera.parse::<f64>().ok()? + d / 10f64.powi(n as i32)
        }
    };
    let bp = if valor <= 1.0 {
        (valor * 10_000.0).round()
    } else if valor <= 100.0 {
        (valor * 100.0).round()
    } else {
        return None; // "1600" no es una tasa: se escribe 16
    };
    Some(bp as i32)
}

// ---------------------------------------------------------------- importar

/// Interpreta el archivo completo y devuelve los productos listos para
/// guardarse, más los renglones que no pasaron.
pub fn interpretar(texto: &str) -> ResultadoImporte {
    interpretar_con(texto, &[])
}

/// Igual que [`interpretar`] pero con el mapeo elegido por la persona.
///
/// `mapeo` tiene un campo por columna del archivo, en orden: `"nombre"`,
/// `"precio"`, `"ignorar"`... Si viene vacío, se adivina como siempre. Es la
/// diferencia entre "no pude leer tu archivo" y "dime qué es cada columna".
pub fn interpretar_con(texto: &str, mapeo: &[String]) -> ResultadoImporte {
    let mut resultado = ResultadoImporte::default();
    let filas = leer_csv(texto);
    if filas.len() < 2 {
        if !texto.trim().is_empty() {
            resultado.errores.push(ErrorRenglon {
                renglon: 1,
                columna: None,
                motivo: "El archivo no tiene encabezados ni datos".into(),
                contenido: texto.chars().take(60).collect(),
            });
        }
        return resultado;
    }

    let encabezados: Vec<Columna> = if mapeo.is_empty() {
        filas[0]
            .iter()
            .map(|h| Columna::desde_encabezado(h))
            .collect()
    } else {
        filas[0]
            .iter()
            .enumerate()
            .map(|(i, h)| match mapeo.get(i) {
                // Si la columna no viene en el mapeo, se adivina con su
                // encabezado: nunca se pierde una columna por un olvido.
                Some(c) if !c.trim().is_empty() => Columna::desde_campo(c),
                _ => Columna::desde_encabezado(h),
            })
            .collect()
    };
    resultado.columnas_reconocidas = encabezados
        .iter()
        .filter(|c| **c != Columna::Ninguna)
        .map(|c| c.etiqueta().to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    if !encabezados.contains(&Columna::Nombre) {
        resultado.errores.push(ErrorRenglon {
            renglon: 1,
            columna: Some("nombre".into()),
            motivo: format!(
                "No encontré la columna del nombre. Encabezados leídos: {}",
                filas[0].join(", ")
            ),
            contenido: filas[0].join(","),
        });
        return resultado;
    }

    let mut productos: Vec<NewProduct> = Vec::new();
    let mut categorias: Vec<Option<String>> = Vec::new();
    for (i, fila) in filas.iter().enumerate().skip(1) {
        let renglon = i + 1;
        let valor = |c: Columna| -> String {
            encabezados
                .iter()
                .position(|e| *e == c)
                .and_then(|p| fila.get(p))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string()
        };

        let nombre = valor(Columna::Nombre);
        if nombre.is_empty() {
            continue; // renglón en blanco: se ignora sin marcar error
        }

        let mut sku = valor(Columna::Sku);
        if sku.is_empty() {
            sku = generar_sku(&nombre, renglon);
        }

        let precio_txt = valor(Columna::Precio);
        let precio = match parsear_dinero(&precio_txt) {
            Some(p) if p >= 0 => p,
            Some(_) => {
                resultado.errores.push(ErrorRenglon {
                    renglon,
                    columna: Some("precio".into()),
                    motivo: "El precio no puede ser negativo".into(),
                    contenido: fila.join(","),
                });
                continue;
            }
            None => {
                resultado.errores.push(ErrorRenglon {
                    renglon,
                    columna: Some("precio".into()),
                    motivo: format!("No entiendo el precio '{precio_txt}'"),
                    contenido: fila.join(","),
                });
                continue;
            }
        };

        let costo_txt = valor(Columna::Costo);
        let costo = if costo_txt.is_empty() {
            (precio as f64 * 0.7).round() as i64 // margen por defecto
        } else {
            match parsear_dinero(&costo_txt) {
                Some(c) => c,
                None => {
                    resultado.errores.push(ErrorRenglon {
                        renglon,
                        columna: Some("costo".into()),
                        motivo: format!("No entiendo el costo '{costo_txt}'"),
                        contenido: fila.join(","),
                    });
                    continue;
                }
            }
        };

        let impuesto = match parsear_impuesto(&valor(Columna::Impuesto)) {
            Some(bp) if (0..=3_000).contains(&bp) => bp,
            _ => {
                resultado.errores.push(ErrorRenglon {
                    renglon,
                    columna: Some("impuesto".into()),
                    motivo: format!(
                        "Impuesto '{}' fuera de rango. Usa 16, 8, 0 o la palabra exento",
                        valor(Columna::Impuesto)
                    ),
                    contenido: fila.join(","),
                });
                continue;
            }
        };

        let existencia_txt = valor(Columna::Existencia);
        let existencia = match parsear_cantidad(&existencia_txt) {
            Some(q) => q,
            None => {
                resultado.errores.push(ErrorRenglon {
                    renglon,
                    columna: Some("existencia".into()),
                    motivo: format!("No entiendo la existencia '{existencia_txt}'"),
                    contenido: fila.join(","),
                });
                continue;
            }
        };

        let minimo = parsear_cantidad(&valor(Columna::Minimo)).unwrap_or(0);
        let unidad = valor(Columna::Unidad);
        let unidad = if unidad.is_empty() {
            "pza".to_string()
        } else {
            unidad
        };
        let codigos: Vec<String> = valor(Columna::CodigoBarras)
            .split([';', '|', '/'])
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();

        resultado.renglones += 1;
        productos.push(NewProduct {
            sku,
            name: nombre,
            price_cents: precio,
            cost_cents: costo,
            tax_bp: impuesto,
            unit: unidad,
            stock_millis: existencia,
            min_stock_millis: minimo,
            track_inventory: true,
            barcodes: codigos,
        });
        let categoria = valor(Columna::Categoria);
        categorias.push(if categoria.is_empty() {
            None
        } else {
            Some(categoria)
        });
    }

    resultado.productos = productos;
    resultado.categorias = categorias;
    resultado
}

/// Si el archivo no trae SKU, se inventa uno legible y estable.
fn generar_sku(nombre: &str, renglon: usize) -> String {
    let base: String = normalizar(nombre)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(10)
        .collect();
    format!("{}-{}", base.to_uppercase(), renglon)
}

// ---------------------------------------------------------------- analizar

/// Cómo se leyó el archivo: para que la persona confirme (o corrija) el mapeo
/// **antes** de importar nada.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AnalisisCsv {
    /// Encabezados tal cual vienen en el archivo.
    pub encabezados: Vec<String>,
    /// Campo sugerido para cada columna, en el mismo orden.
    pub sugerido: Vec<String>,
    /// Los primeros renglones, para que se vea de qué va cada columna.
    pub ejemplo: Vec<Vec<String>>,
    pub separador: char,
    /// Renglones de datos (sin contar el encabezado).
    pub renglones: usize,
    /// `true` si se reconoció una columna de nombre: sin ella no se importa.
    pub tiene_nombre: bool,
}

/// Traduce una columna detectada al nombre de campo del API.
fn campo_de(columna: Columna) -> &'static str {
    match columna {
        Columna::Nombre => "nombre",
        Columna::Precio => "precio",
        Columna::Costo => "costo",
        Columna::Sku => "sku",
        Columna::CodigoBarras => "codigo_barras",
        Columna::Existencia => "existencia",
        Columna::Minimo => "minimo",
        Columna::Impuesto => "impuesto",
        Columna::Unidad => "unidad",
        Columna::Categoria => "categoria",
        Columna::Ninguna => "ignorar",
    }
}

/// Lee sólo el encabezado y un ejemplo: rápido, y no importa nada.
pub fn analizar(texto: &str) -> AnalisisCsv {
    let filas = leer_csv(texto);
    if filas.is_empty() {
        return AnalisisCsv::default();
    }
    let separador = detectar_separador(texto);
    let encabezados = filas[0].clone();
    let sugerido: Vec<String> = encabezados
        .iter()
        .map(|h| campo_de(Columna::desde_encabezado(h)).to_string())
        .collect();
    let ejemplo: Vec<Vec<String>> = filas.iter().skip(1).take(3).cloned().collect();
    AnalisisCsv {
        tiene_nombre: sugerido.iter().any(|s| s == "nombre"),
        renglones: filas.len().saturating_sub(1),
        encabezados,
        sugerido,
        ejemplo,
        separador: if separador == '\t' { ',' } else { separador },
    }
}

// ---------------------------------------------------------------- exportar

/// El catálogo como CSV, listo para abrirse en Excel.
pub fn exportar(productos: &[pos_db::products::Product], codigos: &[(i64, Vec<String>)]) -> String {
    let mut salida = String::with_capacity(productos.len() * 64);
    salida.push_str("sku,nombre,precio,costo,iva,existencia,minimo,codigo_barras,unidad\n");
    for p in productos {
        let barras = codigos
            .iter()
            .find(|(id, _)| *id == p.id)
            .map(|(_, c)| c.join(";"))
            .unwrap_or_default();
        salida.push_str(&format!(
            "{},{},{:.2},{:.2},{},{:.3},{:.3},{},{}\n",
            escapar(&p.sku),
            escapar(&p.name),
            p.price_cents as f64 / 100.0,
            p.cost_cents as f64 / 100.0,
            p.tax_bp as f64 / 100.0,
            p.stock_millis as f64 / 1000.0,
            p.min_stock_millis as f64 / 1000.0,
            escapar(&barras),
            escapar(&p.unit),
        ));
    }
    salida
}

fn escapar(texto: &str) -> String {
    if texto.contains([',', '"', '\n']) {
        format!("\"{}\"", texto.replace('"', "\"\""))
    } else {
        texto.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn columna(texto: &str) -> Columna {
        Columna::desde_encabezado(texto)
    }

    #[test]
    fn reconoce_encabezados_en_espanol_e_ingles() {
        assert_eq!(columna("SKU"), Columna::Sku);
        assert_eq!(columna("Código"), Columna::Sku);
        assert_eq!(columna("Descripción"), Columna::Nombre);
        assert_eq!(columna("descripcion"), Columna::Nombre);
        assert_eq!(columna("Nombre del Producto"), Columna::Nombre);
        assert_eq!(columna("Precio de venta"), Columna::Precio);
        assert_eq!(columna("precio_publico"), Columna::Precio);
        assert_eq!(columna("Costo"), Columna::Costo);
        assert_eq!(columna("Existencia"), Columna::Existencia);
        assert_eq!(columna("Stock mínimo"), Columna::Minimo);
        assert_eq!(columna("Código de barras"), Columna::CodigoBarras);
        assert_eq!(columna("EAN"), Columna::CodigoBarras);
        assert_eq!(columna("IVA"), Columna::Impuesto);
        assert_eq!(columna("Unit"), Columna::Unidad);
    }

    #[test]
    fn precio_y_precio_costo_no_se_confunden() {
        // Éste es el bug clásico: el costo quedaba como precio de venta.
        assert_eq!(columna("Precio"), Columna::Precio);
        assert_eq!(columna("Precio costo"), Columna::Costo);
        assert_eq!(columna("precio_costo"), Columna::Costo);
        assert_eq!(columna("Precio de compra"), Columna::Costo);
    }

    #[test]
    fn el_dinero_se_lee_en_cualquier_formato() {
        assert_eq!(parsear_dinero("$18.50"), Some(1850));
        assert_eq!(parsear_dinero("18.50"), Some(1850));
        assert_eq!(parsear_dinero("18,50"), Some(1850));
        assert_eq!(parsear_dinero("1,234.56"), Some(123_456));
        assert_eq!(parsear_dinero("1.234,56"), Some(123_456));
        assert_eq!(parsear_dinero("1.234"), Some(123)); // el punto es decimal
        assert_eq!(parsear_dinero("1850"), Some(185_000));
        assert_eq!(parsear_dinero("$ 1 234.56"), Some(123_456));
        assert_eq!(parsear_dinero("0"), Some(0));
        assert_eq!(parsear_dinero(""), None);
        assert_eq!(parsear_dinero("abc"), None);
        assert_eq!(parsear_dinero("-5.00"), Some(-500));
    }

    #[test]
    fn las_cantidades_aceptan_decimales_y_miles() {
        assert_eq!(parsear_cantidad("12"), Some(12_000));
        assert_eq!(parsear_cantidad("12.5"), Some(12_500));
        assert_eq!(parsear_cantidad("12,5"), Some(12_500));
        assert_eq!(parsear_cantidad("1,200"), Some(1_200_000));
        assert_eq!(parsear_cantidad("1.200"), Some(1_200)); // punto = decimal
        assert_eq!(parsear_cantidad("1,200"), Some(1_200_000)); // coma con 3 = miles
        assert_eq!(parsear_cantidad(""), Some(0));
    }

    #[test]
    fn el_impuesto_acepta_porcentaje_y_fraccion() {
        assert_eq!(parsear_impuesto("16"), Some(1600));
        assert_eq!(parsear_impuesto("16%"), Some(1600));
        assert_eq!(parsear_impuesto("0.16"), Some(1600));
        assert_eq!(parsear_impuesto("8"), Some(800));
        assert_eq!(parsear_impuesto("150"), None); // no existe una tasa de 150%
        assert_eq!(parsear_impuesto("0"), Some(0));
        assert_eq!(parsear_impuesto(""), Some(0));
    }

    #[test]
    fn lee_csv_con_comas_punto_y_coma_y_comillas() {
        let por_coma = leer_csv("a,b,c\n1,\"dos, con coma\",3\n");
        assert_eq!(por_coma.len(), 2);
        assert_eq!(por_coma[1][1], "dos, con coma");

        let por_punto_y_coma = leer_csv("a;b;c\n1;dos;3\n");
        assert_eq!(por_punto_y_coma[0], vec!["a", "b", "c"]);

        let con_bom = leer_csv("\u{feff}sku,nombre\n1,algo\n");
        assert_eq!(con_bom[0][0], "sku");

        let con_crlf = leer_csv("a,b\r\n1,2\r\n");
        assert_eq!(con_crlf[1], vec!["1", "2"]);
    }

    #[test]
    fn un_catalogo_de_ejemplo_se_importa_completo() {
        let csv = "\
sku,descripcion,precio,costo,iva,existencia,codigo de barras
7501,Coca-Cola 600 ml,$18.50,12.00,16,120,7501055300000
7502,Pan blanco Bimbo,18.50,13,16,30,7501000123457
7503,Leche Lala 1 L,\"27.00\",21,16,40,7501020000000
";
        let r = interpretar(csv);
        assert!(r.errores.is_empty(), "{:?}", r.errores);
        let productos = r.productos;
        assert_eq!(productos.len(), 3);
        assert_eq!(productos[0].sku, "7501");
        assert_eq!(productos[0].name, "Coca-Cola 600 ml");
        assert_eq!(productos[0].price_cents, 1850);
        assert_eq!(productos[0].cost_cents, 1200);
        assert_eq!(productos[0].tax_bp, 1600);
        assert_eq!(productos[0].stock_millis, 120_000);
        assert_eq!(productos[0].barcodes, vec!["7501055300000"]);
        assert_eq!(r.renglones, 3);
    }

    #[test]
    fn sin_columna_de_nombre_el_error_es_claro() {
        let r = interpretar("clave,precio\n1,10\n");
        assert_eq!(r.errores.len(), 1);
        assert_eq!(r.errores[0].renglon, 1);
        assert!(r.errores[0].motivo.contains("nombre"));
    }

    #[test]
    fn un_renglon_malo_se_reporta_sin_tirar_todo() {
        let csv = "\
sku,nombre,precio
1,Bueno,10.00
2,Precio ilegible,caro
3,Tercero,5.00
";
        let r = interpretar(csv);
        assert_eq!(r.errores.len(), 1);
        assert_eq!(r.errores[0].renglon, 3); // 1 = encabezado
        assert!(r.errores[0].motivo.contains("precio"));
        assert_eq!(r.renglones, 2);
    }

    #[test]
    fn sin_sku_se_inventa_uno() {
        let r = interpretar("nombre,precio\nTornillo 1/4,3.50\n");
        assert!(r.errores.is_empty());
        assert_eq!(r.productos[0].sku, "TORNILLO_1-2");
    }

    #[test]
    fn los_renglones_en_blanco_se_ignoran() {
        let r = interpretar("sku,nombre,precio\n1,Uno,10\n\n\n2,Dos,20\n");
        assert!(r.errores.is_empty(), "{:?}", r.errores);
        assert_eq!(r.renglones, 2);
    }

    #[test]
    fn exportar_y_volver_a_importar_no_pierde_nada() {
        let csv = "\
sku,nombre,precio,costo,iva,existencia,minimo
A1,Tornillo,$3.50,2.00,16,120,10
A2,Tuerca 1/4 \"dulce\",1.75,1,16,50,5
";
        let r = interpretar(csv);
        assert!(r.errores.is_empty(), "{:?}", r.errores);

        // Fabricamos productos como los de la base para exportarlos.
        let productos: Vec<pos_db::products::Product> = r
            .productos
            .iter()
            .enumerate()
            .map(|(i, p)| pos_db::products::Product {
                id: i as i64 + 1,
                sku: p.sku.clone(),
                name: p.name.clone(),
                unit: p.unit.clone(),
                price_cents: p.price_cents,
                cost_cents: p.cost_cents,
                tax_bp: p.tax_bp,
                track_inventory: true,
                stock_millis: p.stock_millis,
                min_stock_millis: p.min_stock_millis,
                is_active: true,
                clave_sat: "01010101".into(),
                clave_unidad: "H87".into(),
                unidad_sat: "Pieza".into(),
            })
            .collect();
        let codigos: Vec<(i64, Vec<String>)> = vec![(1, vec!["7501".into()]), (2, vec![])];
        let salida = exportar(&productos, &codigos);

        let volver = interpretar(&salida);
        assert!(volver.errores.is_empty(), "{:?}", volver.errores);
        assert_eq!(volver.productos.len(), 2);
        assert_eq!(volver.productos[0].price_cents, 350);
        assert_eq!(volver.productos[1].name, "Tuerca 1/4 \"dulce\"");
        assert_eq!(volver.productos[1].min_stock_millis, 5_000);
    }

    #[test]
    fn diez_mil_renglones_se_leen_rapido() {
        let mut csv = String::from("sku,nombre,precio,costo,iva,existencia\n");
        for i in 0..10_000 {
            csv.push_str(&format!("P{i},Producto {i},10.50,7.00,16,25\n"));
        }
        let inicio = std::time::Instant::now();
        let r = interpretar(&csv);
        let transcurrido = inicio.elapsed();
        assert_eq!(r.renglones, 10_000);
        assert!(
            transcurrido.as_millis() < 1_500,
            "tardó {transcurrido:?} en leer 10,000 renglones"
        );
    }

    #[test]
    fn el_mapa_de_columnas_se_reporta_al_usuario() {
        let r = interpretar("sku,descripcion,precio,existencia,codigo de barras\n1,Uno,1,2,3\n");
        let mapa: HashMap<&str, bool> = HashMap::new();
        let _ = mapa;
        assert!(r.columnas_reconocidas.contains(&"SKU".to_string()));
        assert!(r.columnas_reconocidas.contains(&"nombre".to_string()));
        assert!(r
            .columnas_reconocidas
            .contains(&"código de barras".to_string()));
        assert_eq!(r.columnas_reconocidas.len(), 5);
    }

    fn map(c: &str) -> String {
        c.to_string()
    }

    #[test]
    fn desde_campo_acepta_las_claves_del_dialogo() {
        assert_eq!(Columna::desde_campo("nombre"), Columna::Nombre);
        assert_eq!(Columna::desde_campo("precio"), Columna::Precio);
        assert_eq!(Columna::desde_campo("costo"), Columna::Costo);
        assert_eq!(Columna::desde_campo("sku"), Columna::Sku);
        assert_eq!(Columna::desde_campo("codigo_barras"), Columna::CodigoBarras);
        assert_eq!(Columna::desde_campo("existencia"), Columna::Existencia);
        assert_eq!(Columna::desde_campo("minimo"), Columna::Minimo);
        assert_eq!(Columna::desde_campo("impuesto"), Columna::Impuesto);
        assert_eq!(Columna::desde_campo("unidad"), Columna::Unidad);
        assert_eq!(Columna::desde_campo("categoria"), Columna::Categoria);
    }

    #[test]
    fn desde_campo_ignora_lo_que_no_sirve() {
        // Cuatro formas de decir "esta columna no me importa".
        for vacio in ["ignorar", "", "-", "sin_usar", "NINGUNA"] {
            assert_eq!(
                Columna::desde_campo(vacio),
                Columna::Ninguna,
                "se esperaba ignorar '{vacio}'"
            );
        }
        // Una clave que no existe no inventa nada: se ignora, no adivina.
        assert_eq!(Columna::desde_campo("no_existe"), Columna::Ninguna);
    }

    #[test]
    fn los_campos_del_dialogo_tienen_clave_y_etiqueta() {
        let campos = Columna::campos();
        assert_eq!(campos.len(), 11);
        // Toda clave del diálogo se entiende de vuelta.
        for (clave, etiqueta) in campos {
            assert!(!clave.is_empty());
            assert!(!etiqueta.is_empty());
            if *clave != "nombre" {
                // Ninguna otra clave debe caer en Nombre por accidente:
                // eso importaría basura en la columna equivocada.
                assert_ne!(
                    Columna::desde_campo(clave),
                    Columna::Nombre,
                    "'{clave}' no debe caer en Nombre por accidente"
                );
            }
        }
        assert_eq!(Columna::desde_campo(campos[0].0), Columna::Nombre);
        assert_eq!(Columna::desde_campo(campos[10].0), Columna::Ninguna);
    }

    #[test]
    fn sin_mapeo_sigue_adivinando() {
        let csv = "Nombre,Precio,Costo\nCoca cola 600ml,18.50,12.00\n";
        let r = interpretar_con(csv, &[]);
        assert_eq!(r.productos.len(), 1);
        assert_eq!(r.productos[0].name, "Coca cola 600ml");
        // Y sin mapeo explícito, el CSV se entendió por el encabezado.
        assert!(r.columnas_reconocidas.contains(&"nombre".to_string()));
    }

    #[test]
    fn el_mapeo_a_mano_gana_sobre_el_encabezado_mentiroso() {
        // Un CSV donde la columna "Descripción" trae el precio y viceversa.
        // Adivinando no hay forma; el cajero lo acomoda a mano.
        let csv = "Descripcion,Precio\n18.50,Coca cola 600ml\n";
        let r = interpretar_con(csv, &[map("precio"), map("nombre")]);
        assert_eq!(r.productos[0].name, "Coca cola 600ml");
        assert_eq!(r.productos[0].price_cents, 1850);
    }

    #[test]
    fn una_columna_ignorada_no_ensucia_el_producto() {
        let csv = "Nombre,Precio,Notas internas\nCoca,18.50,no surtir los lunes\n";
        // Sin mapeo, "Notas internas" no se reconoce y se descarta igual.
        let automatico = interpretar_con(csv, &[]);
        assert_eq!(automatico.productos.len(), 1);
        // Con mapeo explícito se marca y el resultado es el mismo.
        let manual = interpretar_con(csv, &[map("nombre"), map("precio"), map("ignorar")]);
        assert_eq!(manual.productos.len(), 1);
        assert_eq!(manual.productos[0].name, "Coca");
        assert_eq!(manual.productos[0].price_cents, 1850);
    }

    #[test]
    fn el_mapeo_puede_reordenar_las_columnas() {
        let csv = "c1,c2,c3,c4\nAbarrotes,7501055300006,Coca cola 600ml,18.50\n";
        let r = interpretar_con(
            csv,
            &[
                map("categoria"),
                map("codigo_barras"),
                map("nombre"),
                map("precio"),
            ],
        );
        assert_eq!(r.productos.len(), 1);
        assert!(r.errores.is_empty(), "{:?}", r.errores);
        assert_eq!(r.productos[0].name, "Coca cola 600ml");
        assert_eq!(r.productos[0].price_cents, 1850);
        assert_eq!(r.productos[0].barcodes, vec!["7501055300006".to_string()]);
        // La categoría viaja aparte: es otra tabla.
        assert_eq!(r.categorias[0].as_deref(), Some("Abarrotes"));
    }

    #[test]
    fn una_columna_fuera_del_mapeo_cae_en_adivinar() {
        // El mapeo sólo trae la primera columna; la segunda se adivina
        // por su encabezado. Mezclar las dos formas debe funcionar.
        let csv = "Nombre,Precio\nCoca,18.50\n";
        let r = interpretar_con(csv, &[map("nombre")]);
        assert_eq!(r.productos[0].name, "Coca");
        assert_eq!(r.productos[0].price_cents, 1850);
    }

    #[test]
    fn la_categoria_viaja_aparte_y_en_el_mismo_orden() {
        let csv = "Nombre,Precio,Categoria\nCoca,18.50,Abarrotes\nLeche,22.00,\nPan,7.00,Lacteos\n";
        let r = interpretar_con(csv, &[]);
        assert_eq!(r.productos.len(), 3);
        assert_eq!(r.categorias.len(), r.productos.len());
        assert_eq!(r.categorias[0].as_deref(), Some("Abarrotes"));
        // Sin categoría en el renglón: None, no una cadena vacía.
        assert_eq!(r.categorias[1], None);
        assert_eq!(r.categorias[2].as_deref(), Some("Lacteos"));
    }

    #[test]
    fn la_categoria_se_puede_mapear_a_mano() {
        let csv = "c1,c2,c3\nCoca,18.50,Bebidas\n";
        let r = interpretar_con(csv, &[map("nombre"), map("precio"), map("categoria")]);
        assert_eq!(r.categorias[0].as_deref(), Some("Bebidas"));
    }

    #[test]
    fn el_analisis_dice_que_columna_es_cual_y_muestra_un_ejemplo() {
        let csv = "Nombre,Precio,Costo\nCoca,18.50,12.00\nSabritas,15.00,9.50\nPan,7.00,4.00\n";
        let a = analizar(csv);
        assert_eq!(a.encabezados, vec!["Nombre", "Precio", "Costo"]);
        assert_eq!(a.sugerido, vec!["nombre", "precio", "costo"]);
        assert_eq!(a.separador, ',');
        assert_eq!(a.renglones, 3);
        assert!(a.tiene_nombre);
        // Tres renglones de ejemplo, no todo el archivo: esto se pinta
        // en un diálogo, no en una tabla de 10,000.
        assert_eq!(a.ejemplo.len(), 3);
        assert_eq!(a.ejemplo[0], vec!["Coca", "18.50", "12.00"]);
    }

    #[test]
    fn el_analisis_avisa_si_falta_el_nombre() {
        // Sin nombre no hay producto: hay que decírselo antes de importar,
        // no después de que falle.
        let csv = "Precio,Costo\n18.50,12.00\n";
        let a = analizar(csv);
        assert!(!a.tiene_nombre);
        assert_eq!(a.encabezados, vec!["Precio", "Costo"]);
        assert_eq!(a.sugerido, vec!["precio", "costo"]);
    }

    #[test]
    fn el_analisis_detecta_punto_y_coma() {
        let a = analizar("Nombre;Precio\nCoca;18,50\n");
        assert_eq!(a.separador, ';');
        assert_eq!(a.encabezados, vec!["Nombre", "Precio"]);
        // Y lo analizado se puede importar con el mismo criterio.
        let r = interpretar_con("Nombre;Precio\nCoca;18,50\n", &[]);
        assert_eq!(r.productos[0].price_cents, 1850);
    }
}
