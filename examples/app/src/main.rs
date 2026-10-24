//! An app that adds rawr. It exports its own `Order` type and a function,
//! then loads a script and calls the script's functions.
//! `scripts/embed-check.sh` runs it.

use rawr::{Engine, Exports, Scope};

#[derive(Clone)]
#[rawr::export]
pub struct Order {
    pub customer: String,
    pub items: i64,
}

#[rawr::export]
impl Order {
    pub fn total(&self, price: f64) -> f64 {
        price * self.items as f64
    }

    pub fn add(&mut self, n: i64) {
        self.items += n;
    }
}

#[rawr::export]
fn tax_rate(state: &str) -> f64 {
    if state == "NY" { 0.08875 } else { 0.0 }
}

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    // Scripts are named relative to the app, as rustc names source files.
    std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).expect("the app's directory");
    let script = std::env::args().nth(1).unwrap_or("scripts/pricing.rs".into());

    let mut shop = Exports::new("shop");
    shop.add::<Order>().add::<tax_rate>();
    let mut engine = Engine::new();
    engine.export(shop)?;

    let ast = engine.compile_file(script.into())?;
    let mut scope = Scope::new();

    let total = engine.call_fn::<f64>(&mut scope, &ast, "price", (100.0_f64, 2_i64))?;
    println!("{total:.2}");
    let receipt = engine.call_fn::<String>(&mut scope, &ast, "receipt", (100.0_f64, 5_i64))?;
    println!("{receipt}");

    let order = Order { customer: "Ada".into(), items: 2 };
    let label = engine.call_fn::<String>(&mut scope, &ast, "label", (order.clone(),))?;
    println!("{label}");
    let bigger = engine.call_fn::<f64>(&mut scope, &ast, "bigger", (order,))?;
    println!("{bigger:.2}");
    Ok(())
}
