fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let mut __o = Some(&x);
    __see(&__o);
    __o = None;
    println!("{}", *r + x);
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
