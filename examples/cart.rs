//! Rawr without a shell: Rust code evaluating Rust-shaped script.
use rawr::Engine;

const SCRIPT: &str = r#"
enum Item {
    Book { price: f64 },
    Coffee { cups: i64 },
}

trait Priced {
    fn price(&self) -> f64;
}

impl Priced for Item {
    fn price(&self) -> f64 {
        match self {
            Item::Book { price } => *price,
            Item::Coffee { cups } => 3.5 * (*cups as f64),
        }
    }
}

fn total(items: &[Item]) -> f64 {
    let mut sum = 0.0;
    for item in items {
        sum += item.price();
    }
    sum
}

let cart = vec![Item::Book { price: 20.0 }, Item::Coffee { cups: 2 }];
total(&cart)
"#;

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let total = Engine::new().eval::<f64>(SCRIPT)?;
    println!("total: {total}");
    Ok(())
}
