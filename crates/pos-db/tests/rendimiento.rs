//! Medición, no verificación.
//!
//! Estas pruebas no comprueban nada: **miden**. Sirven para saber si un cambio
//! de verdad mejoró algo o nomás se sintió más rápido. Se corren aparte:
//!
//!     cargo test -p pos-db --test rendimiento -- --ignored --nocapture

use std::time::Instant;

use pos_db::products::NewProduct;
use pos_db::{migrations, products, Database};

fn tienda(cuantos: i64) -> Database {
    let mut db = Database::open_in_memory().unwrap();
    migrations::migrate(db.conn()).unwrap();
    for i in 0..cuantos {
        products::Products::insert(
            db.conn_mut(),
            &NewProduct {
                sku: format!("SKU{i:06}"),
                name: format!("Producto de prueba número {i}"),
                price_cents: 1_800,
                cost_cents: 900,
                tax_bp: 1600,
                unit: "pza".into(),
                stock_millis: 100_000,
                min_stock_millis: 0,
                track_inventory: true,
                barcodes: vec![format!("750{i:09}")],
            },
        )
        .unwrap();
    }
    db
}

/// Lo que hace el cajero todo el día: escribir y que la lista se filtre.
#[test]
#[ignore = "mide, no verifica"]
fn dos_mil_busquedas_de_catalogo() {
    let mut db = tienda(5_000);
    let conn = db.conn_mut();
    let inicio = Instant::now();
    let mut encontrados = 0;
    for i in 0..2_000 {
        let termino = format!("producto {}", i % 500);
        encontrados += products::Products::search(conn, &termino, 48)
            .unwrap()
            .len();
    }
    let t = inicio.elapsed();
    println!(
        "2,000 búsquedas sobre 5,000 productos: {:?} ({:?} por búsqueda) · {encontrados} resultados",
        t,
        t / 2_000
    );
}

/// Lo que hace el escáner: una lectura tras otra, todo el día.
#[test]
#[ignore = "mide, no verifica"]
fn cinco_mil_lecturas_de_codigo() {
    let mut db = tienda(5_000);
    let conn = db.conn_mut();
    let inicio = Instant::now();
    for i in 0..5_000 {
        let _ = products::Products::by_barcode(conn, &format!("750{i:09}")).unwrap();
    }
    let t = inicio.elapsed();
    println!(
        "5,000 lecturas de código de barras: {:?} ({:?} por lectura)",
        t,
        t / 5_000
    );
}

/// Devolver y reimprimir: se busca el folio, se busca otra vez, se busca...
#[test]
#[ignore = "mide, no verifica"]
fn dos_mil_busquedas_por_folio() {
    let db = tienda(200);
    let conn = db.conn();
    let inicio = Instant::now();
    for _ in 0..2_000 {
        let _ = pos_db::sales::Sales::by_folio(conn, "A-1").unwrap();
    }
    let t = inicio.elapsed();
    println!(
        "2,000 búsquedas por folio: {:?} ({:?} por búsqueda)",
        t,
        t / 2_000
    );
}
