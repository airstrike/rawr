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
    let __b = &mut bag;
    for __i in 0..2 {
        __touch(__b);
        println!("{:?}", bag.items);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
