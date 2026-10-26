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
    let __k = __keep(&bag);
    __see(__k);
    bag.items.push(3);
    println!("{}", bag.items.len());
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
