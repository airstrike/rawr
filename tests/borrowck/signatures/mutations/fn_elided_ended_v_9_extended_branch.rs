fn first(v: &Vec<i32>) -> &i32 {
    &v[0]
}

fn main() {
    let mut v = vec![1, 2, 3];
    let f = first(&v);
    println!("{f}");
    let __t = if __c() { &__id(&v) } else { &__id(&v) };
    v.push(4);
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
