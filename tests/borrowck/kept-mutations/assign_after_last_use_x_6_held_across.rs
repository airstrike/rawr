fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    let __h = __Hold(&x);
    println!("{}", x + y);
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
