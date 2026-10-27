use std::collections::HashMap;

fn main() {
    let mut m = HashMap::new();
    __see(&vec![&m].first());
    m.insert(1, String::from("one"));
    let one = m.get(&1);
    m.insert(2, String::from("two"));
    println!("{one:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
