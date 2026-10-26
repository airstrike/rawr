use std::collections::HashMap;

fn main() {
    let mut a = String::from("v");
    let mut m = HashMap::new();
    m.insert(1, &a);
    a.push('!');
    let __o = Some(&m);
    println!("{m:?}");
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
