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
    let __t = vec![&bag].first();
    bag.items.push(3);
    __see(&__t);
    println!("{f}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
