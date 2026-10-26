fn main() {
    let mut v = vec![1, 2];
    let n = v.len();
    let __o = Some(&v);
    v.push(3);
    __see(&__o);
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
