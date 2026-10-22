// rawr
mod pricing;

pub struct Item {
    pub cents: i64,
}

fn main() {
    let item = Item { cents: 1250 };
    println!("{}", pricing::doubled(&item));
}
