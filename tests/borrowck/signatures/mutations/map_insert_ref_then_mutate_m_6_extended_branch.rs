use std::collections::HashMap;

fn main() {
    let mut a = String::from("v");
    let mut m = HashMap::new();
    let __t = if __c() { &__id(&m) } else { &__id(&m) };
    m.insert(1, &a);
    __see(__t);
    a.push('!');
    println!("{m:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
