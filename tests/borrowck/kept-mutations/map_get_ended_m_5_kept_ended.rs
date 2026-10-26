use std::collections::HashMap;

fn main() {
    let mut m = HashMap::new();
    let __k = __keep(&m);
    __see(__k);
    m.insert(1, String::from("one"));
    let one = m.get(&1).cloned();
    m.insert(2, String::from("two"));
    println!("{one:?} {}", m.len());
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
