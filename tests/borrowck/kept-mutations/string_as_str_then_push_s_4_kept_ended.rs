fn main() {
    let mut s = String::from("hi");
    let t = s.as_str();
    let __k = __keep(&s);
    __see(__k);
    s.push_str("!");
    println!("{t}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
