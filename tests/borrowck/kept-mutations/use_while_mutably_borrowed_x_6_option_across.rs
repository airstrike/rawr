fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    let __o = Some(&x);
    println!("{} {}", x, y);
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
