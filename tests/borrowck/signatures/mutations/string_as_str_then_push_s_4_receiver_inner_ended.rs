fn main() {
    let mut s = String::from("hi");
    let t = s.as_str();
    let __t = vec![&s].to_vec();
    __see(&__t);
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
