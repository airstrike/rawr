fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __k = __keep(&v);
    v[0] = 5;
    __see(__k);
    println!("{} {:?}", first, v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
