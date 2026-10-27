use std::collections::HashMap;

fn main() {
    let mut m = HashMap::new();
    m.insert(1, String::from("one"));
    let one = m.get(&1).cloned();
    let __t = &__id(&m);
    __see(__t);
    m.insert(2, String::from("two"));
    println!("{one:?} {}", m.len());
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
