fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __v = vec![&x];
    println!("{}", *r + x);
    __see(&__v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
