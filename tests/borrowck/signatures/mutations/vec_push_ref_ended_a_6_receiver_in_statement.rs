fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    refs.push(&a);
    println!("{refs:?}");
    __see(&vec![&a].first());
    a += 1;
    println!("{a}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
