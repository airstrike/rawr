fn main() {
    let mut v = vec![10, 20];
    let x = v.get(1).unwrap();
    let __k = __keep(&v);
    v.clear();
    __see(__k);
    println!("{x}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
