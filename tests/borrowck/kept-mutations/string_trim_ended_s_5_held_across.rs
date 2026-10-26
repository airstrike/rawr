fn main() {
    let mut s = String::from("  hi  ");
    let t = s.trim();
    println!("[{t}]");
    let __h = __Hold(&s);
    s.push_str("!");
    __see(&__h);
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
