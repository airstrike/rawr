fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    __see(&vec![&v].first());
    v.push(4);
    println!("{f:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
