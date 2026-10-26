use std::collections::HashMap;

fn main() {
    let mut a = String::from("v");
    let mut m = HashMap::new();
    let __k = __keep(&m);
    for __i in 0..2 {
        __see(__k);
        m.insert(1, &a);
    }
    a.push('!');
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
