fn main() {
    let text = String::from(" a ");
    let other = String::from("b");
    let mut s: &str = &text;
    let t = s.trim();
    __see(&vec![&s].first());
    s = &other;
    println!("{t} {s}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
