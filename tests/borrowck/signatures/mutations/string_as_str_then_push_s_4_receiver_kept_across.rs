fn main() {
    let mut s = String::from("hi");
    let t = s.as_str();
    let __t = vec![&s].first();
    s.push_str("!");
    __see(&__t);
    println!("{t}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
