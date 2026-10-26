use std::collections::HashMap;

fn main() {
    let mut m = HashMap::new();
    m.insert(1, String::from("one"));
    let one = m.get(&1).cloned();
    let mut __o = Some(&m);
    __see(&__o);
    __o = None;
    m.insert(2, String::from("two"));
    __see(&__o);
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
