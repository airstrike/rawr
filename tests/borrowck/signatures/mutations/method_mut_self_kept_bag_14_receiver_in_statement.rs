struct Bag {
    items: Vec<i32>,
}

impl Bag {
    fn slot(&mut self) -> &mut i32 {
        &mut self.items[0]
    }
}

fn main() {
    let mut bag = Bag { items: vec![1, 2] };
    let s = bag.slot();
    __see(&vec![&bag].first());
    println!("{}", bag.items.len());
    *s = 5;
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
