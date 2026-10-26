fn main() {
    let mut x = 1;
    let r = &x;
    let __k = __keep(&x);
    x = 2;
    if __c() {
        __see(__k);
    }
    println!("{}", *r + x);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
