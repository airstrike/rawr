fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __h = __Hold(&v);
    v.push(2);
    __see(&__h);
    println!("{}", s.len());
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
