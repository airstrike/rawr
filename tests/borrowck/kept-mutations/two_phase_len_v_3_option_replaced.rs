fn main() {
    let mut v = vec![1, 2];
    let mut __o = Some(&v);
    __see(&__o);
    __o = None;
    v.push(v.len());
    __see(&__o);
    let r = &v;
    v.push(r.len());
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
