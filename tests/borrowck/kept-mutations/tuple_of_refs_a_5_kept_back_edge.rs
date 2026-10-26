fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    let __k = __keep(&a);
    for __i in 0..2 {
        __see(__k);
        a += 1;
    }
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
