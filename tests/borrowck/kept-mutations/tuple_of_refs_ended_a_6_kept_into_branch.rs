fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    println!("{} {}", pair.0, pair.1);
    let __k = __keep(&a);
    a += 1;
    if __c() {
        __see(__k);
    }
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
