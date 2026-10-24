//! rawr embedded: a Rust program that loads a script written in Rust.
use rawr::{Engine, Scope};

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let engine = Engine::new();
    let ast = engine.compile(include_str!("scripts/cart.rs"))?;
    engine.call_fn::<()>(&mut Scope::new(), &ast, "main", ())
}
