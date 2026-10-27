fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __t = if __c() { &__id(&s) } else { &__id(&s) };
    s.push('b');
    __see(__t);
    println!("{n} {s}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
