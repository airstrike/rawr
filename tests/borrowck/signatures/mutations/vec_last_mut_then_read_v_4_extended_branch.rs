fn main() {
    let mut v = vec![1, 2, 3];
    let last = v.last_mut().unwrap();
    let __t = if __c() { &__id(&v) } else { &__id(&v) };
    println!("{}", v.len());
    __see(__t);
    *last = 9;
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
