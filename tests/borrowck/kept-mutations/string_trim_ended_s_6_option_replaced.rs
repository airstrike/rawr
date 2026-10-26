fn main() {
    let mut s = String::from("  hi  ");
    let t = s.trim();
    println!("[{t}]");
    s.push_str("!");
    let mut __o = Some(&s);
    __see(&__o);
    __o = None;
    println!("[{s}]");
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
