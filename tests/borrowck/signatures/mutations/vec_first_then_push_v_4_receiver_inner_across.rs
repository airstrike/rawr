fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    let __t = vec![&v].to_vec();
    v.push(4);
    __see(&__t);
    println!("{f:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
