// rawr
pub enum Item {
    Book { price: f64 },
    Coffee { cups: i64 },
}

impl Item {
    pub fn price(&self) -> f64 {
        match self {
            Item::Book { price } => *price,
            Item::Coffee { cups } => 3.5 * (*cups as f64),
        }
    }
}
