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
    let __k = __keep(&bag);
    for __i in 0..2 {
        __see(__k);
        println!("{}", bag.items.len());
    }
    *s = 5;
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
