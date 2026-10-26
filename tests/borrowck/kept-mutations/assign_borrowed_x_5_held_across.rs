fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __h = __Hold(&x);
    println!("{}", *r + x);
    __see(&__h);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
