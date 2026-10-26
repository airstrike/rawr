use std::collections::HashMap;

fn main() {
    let mut m = HashMap::new();
    let mut __o = Some(&m);
    __see(&__o);
    __o = None;
    m.insert(1, String::from("one"));
    __see(&__o);
    let one = m.get(&1);
    m.insert(2, String::from("two"));
    println!("{one:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
