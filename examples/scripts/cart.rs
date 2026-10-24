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

fn main() {
    let cart = vec![Item::Book { price: 20.0 }, Item::Coffee { cups: 2 }];
    println!("total: {}", total(&cart));
}
