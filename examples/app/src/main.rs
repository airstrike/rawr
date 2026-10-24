//! An app that adds Rawr: it registers a function, then loads a script and
//! calls the script's functions. `scripts/embed-check.sh` runs it.

use rawr::{Engine, Scope};

fn tax_rate(state: &str) -> f64 {
    if state == "NY" { 0.08875 } else { 0.0 }
}

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let mut engine = Engine::new();
    engine.register_fn("tax_rate", tax_rate);

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/pricing.rawr");
    let source = std::fs::read_to_string(path).expect("the pricing script");
    let ast = engine.compile(&source)?;
    let mut scope = Scope::new();

    let total = engine.call_fn::<f64>(&mut scope, &ast, "price", (100.0_f64, 2_i64))?;
    println!("{total:.2}");
    let receipt = engine.call_fn::<String>(&mut scope, &ast, "receipt", (100.0_f64, 5_i64))?;
    println!("{receipt}");
    Ok(())
}
