fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let mut __o = Some(&x);
    __see(&__o);
    __o = None;
    x = 2;
    __see(&__o);
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
