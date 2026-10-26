fn main() {
    let mut v = vec![1, 2];
    let w = v.to_vec();
    let __h = __Hold(&v);
    v.push(3);
    __see(&__h);
    println!("{w:?} {v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
