fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __t = &__id(&s);
    s.push('b');
    __see(__t);
    println!("{r}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
