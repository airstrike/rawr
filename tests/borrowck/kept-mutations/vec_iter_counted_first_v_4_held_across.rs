fn main() {
    let mut v = vec![1, 2];
    let n = v.iter().count();
    let __h = __Hold(&v);
    v.push(3);
    __see(&__h);
    println!("{n} {v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
