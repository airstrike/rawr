struct Bag {
    items: Vec<i32>,
}

impl Bag {
    fn first(&self) -> &i32 {
        &self.items[0]
    }
}

fn main() {
    let mut bag = Bag { items: vec![1, 2] };
    let f = bag.first();
    println!("{f}");
    bag.items.push(3);
    println!("{}", bag.items.len());
}
