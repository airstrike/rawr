fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    let __k = __keep(&v);
    __see(__k);
    v.push(4);
    println!("{f:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
