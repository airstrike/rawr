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
    let __h = __Hold(&bag);
    bag.items.push(3);
    __see(&__h);
    println!("{f}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
