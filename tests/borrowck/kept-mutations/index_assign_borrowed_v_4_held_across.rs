fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __h = __Hold(&v);
    v[0] = 5;
    __see(&__h);
    println!("{}", r[0]);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
