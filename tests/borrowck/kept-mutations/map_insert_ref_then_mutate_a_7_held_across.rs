use std::collections::HashMap;

fn main() {
    let mut a = String::from("v");
    let mut m = HashMap::new();
    m.insert(1, &a);
    let __h = __Hold(&a);
    a.push('!');
    __see(&__h);
    println!("{m:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
