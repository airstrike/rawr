fn main() {
    let mut v = vec![1, 2];
    for x in v.iter() {
        if *x > 5 {
            v.push(*x);
        }
    }
    let __t = &__id(&v);
    __see(__t);
    println!("{v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
