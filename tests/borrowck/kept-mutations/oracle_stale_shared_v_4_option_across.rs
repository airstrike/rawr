fn main() {
    let mut v = vec![1];
    let r = &v;
    let __o = Some(&v);
    v.push(1);
    __see(&__o);
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
