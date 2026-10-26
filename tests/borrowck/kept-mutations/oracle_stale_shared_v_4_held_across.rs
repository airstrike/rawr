fn main() {
    let mut v = vec![1];
    let r = &v;
    let __h = __Hold(&v);
    v.push(1);
    __see(&__h);
    println!("{r:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
