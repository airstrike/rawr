//! Rawr embedded: Rust code running a Rust-shaped script.
use rawr::{Engine, Scope};

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let engine = Engine::new();
    let ast = engine.compile(include_str!("scripts/cart.rawr"))?;
    engine.call_fn::<()>(&mut Scope::new(), &ast, "main", ())
}
