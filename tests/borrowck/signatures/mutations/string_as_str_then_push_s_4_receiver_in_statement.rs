fn main() {
    let mut s = String::from("hi");
    let t = s.as_str();
    __see(&vec![&s].first());
    s.push_str("!");
    println!("{t}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
