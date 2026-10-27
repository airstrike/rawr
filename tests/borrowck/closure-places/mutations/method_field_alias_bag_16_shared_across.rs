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
    let __s = &bag;
    bag.fill();
    __see(__s);
    println!("{:?}", bag.items);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
