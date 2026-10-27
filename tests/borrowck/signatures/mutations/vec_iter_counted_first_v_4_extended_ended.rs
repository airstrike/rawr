fn main() {
    let mut v = vec![1, 2];
    let n = v.iter().count();
    let __t = &__id(&v);
    __see(__t);
    v.push(3);
    println!("{n} {v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
