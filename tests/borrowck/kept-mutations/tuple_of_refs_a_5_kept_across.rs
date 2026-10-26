fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    let __k = __keep(&a);
    a += 1;
    __see(__k);
    println!("{}", pair.1);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
