fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let __k = __keep(&x);
    for __i in 0..2 {
        __see(__k);
        x = 2;
    }
    println!("{}", x + y);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
