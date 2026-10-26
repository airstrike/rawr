fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    println!("{f:?}");
    v.push(4);
    let __k = __keep(&v);
    println!("{v:?}");
    __see(__k);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
