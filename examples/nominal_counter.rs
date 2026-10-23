use rawr::Engine;

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let value = Engine::new().eval::<i64>(include_str!("scripts/nominal_counter.rs"))?;

    println!("Rawr nominal counter: {value}");
    Ok(())
}
