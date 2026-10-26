fn main() {
    let mut x = 1;
    let r = &x;
    let __h = __Hold(&x);
    x = 2;
    __see(&__h);
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
