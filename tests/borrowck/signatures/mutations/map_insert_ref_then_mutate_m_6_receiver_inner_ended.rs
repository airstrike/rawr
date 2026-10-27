use std::collections::HashMap;

fn main() {
    let mut a = String::from("v");
    let mut m = HashMap::new();
    let __t = vec![&m].to_vec();
    __see(&__t);
    m.insert(1, &a);
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
