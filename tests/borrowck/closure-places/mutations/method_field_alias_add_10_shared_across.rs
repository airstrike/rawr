struct Bag {
    items: Vec<i32>,
}

impl Bag {
    fn fill(&mut self) {
        let items = &mut self.items;
        let mut add = |x| items.push(x);
        add(1);
        let __s = &add;
        add(2);
        __see(__s);
    }
}

fn main() {
    let mut bag = Bag { items: vec![] };
    bag.fill();
    println!("{:?}", bag.items);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
