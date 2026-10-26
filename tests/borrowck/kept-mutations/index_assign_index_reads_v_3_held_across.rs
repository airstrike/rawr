fn main() {
    let mut v = vec![1, 2];
    let __h = __Hold(&v);
    v[v.len() - 1] = 5;
    __see(&__h);
    println!("{:?}", v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
