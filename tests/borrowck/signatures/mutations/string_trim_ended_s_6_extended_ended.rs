fn main() {
    let mut s = String::from("  hi  ");
    let t = s.trim();
    println!("[{t}]");
    s.push_str("!");
    let __t = &__id(&s);
    __see(__t);
    println!("[{s}]");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
