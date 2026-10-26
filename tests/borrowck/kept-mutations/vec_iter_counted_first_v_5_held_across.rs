fn main() {
    let mut v = vec![1, 2];
    let n = v.iter().count();
    v.push(3);
    let __h = __Hold(&v);
    println!("{n} {v:?}");
    __see(&__h);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
