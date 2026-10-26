fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __o = Some(&v);
    v[0] = 5;
    __see(&__o);
    println!("{} {:?}", first, v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
