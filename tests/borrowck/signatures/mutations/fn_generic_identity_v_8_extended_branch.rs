fn id<T>(x: T) -> T {
    x
}

fn main() {
    let mut v = vec![1];
    let r = id(&v);
    let __t = if __c() { &__id(&v) } else { &__id(&v) };
    v.push(2);
    __see(__t);
    println!("{r:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
