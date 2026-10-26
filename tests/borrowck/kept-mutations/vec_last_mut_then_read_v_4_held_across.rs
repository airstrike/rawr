fn main() {
    let mut v = vec![1, 2, 3];
    let last = v.last_mut().unwrap();
    let __h = __Hold(&v);
    println!("{}", v.len());
    __see(&__h);
    *last = 9;
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
