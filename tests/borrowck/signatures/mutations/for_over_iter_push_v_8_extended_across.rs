fn main() {
    let mut v = vec![1, 2];
    for x in v.iter() {
        if *x > 5 {
            v.push(*x);
        }
    }
    let __t = &__id(&v);
    println!("{v:?}");
    __see(__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
