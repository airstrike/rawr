fn main() {
    let mut a = 1;
    let b = 2;
    let refs = vec![&a, &b];
    println!("{refs:?}");
    let __k = __keep(&a);
    a = 5;
    __see(__k);
    println!("{a}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
