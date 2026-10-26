fn main() {
    let mut s = String::from("  hi  ");
    let t = s.trim();
    println!("[{t}]");
    let __k = __keep(&s);
    __see(__k);
    s.push_str("!");
    println!("[{s}]");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
