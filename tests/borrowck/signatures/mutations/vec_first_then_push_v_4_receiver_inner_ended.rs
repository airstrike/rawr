fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    let __t = vec![&v].to_vec();
    __see(&__t);
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
