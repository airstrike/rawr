fn main() {
    let mut v = vec![10, 20];
    let x = v.get(1).unwrap();
    let __k = __keep(&v);
    for __i in 0..2 {
        __see(__k);
        v.clear();
    }
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
