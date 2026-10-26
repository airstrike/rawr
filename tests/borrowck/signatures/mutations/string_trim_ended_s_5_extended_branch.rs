fn main() {
    let mut s = String::from("  hi  ");
    let t = s.trim();
    println!("[{t}]");
    let __t = if __c() { &__id(&s) } else { &__id(&s) };
    s.push_str("!");
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
