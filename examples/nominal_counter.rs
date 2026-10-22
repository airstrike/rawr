use rawr::Engine;

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let value = Engine::new().eval::<i64>(include_str!("nominal_counter.rawr"))?;

    println!("Rawr nominal counter: {value}");
    Ok(())
}
