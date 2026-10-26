fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    println!("{f:?}");
    let mut __o = Some(&v);
    __see(&__o);
    __o = None;
    v.push(4);
    __see(&__o);
    println!("{v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
