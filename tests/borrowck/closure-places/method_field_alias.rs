struct Bag {
    items: Vec<i32>,
}

impl Bag {
    fn fill(&mut self) {
        let items = &mut self.items;
        let mut add = |x| items.push(x);
        add(1);
        add(2);
    }
}

fn main() {
    let mut bag = Bag { items: vec![] };
    bag.fill();
    println!("{:?}", bag.items);
}
