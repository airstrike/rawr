fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    v[0] = 5;
    let __o = Some(&v);
    println!("{} {:?}", first, v);
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
