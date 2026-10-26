fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __o = Some(&v);
    r.push(v.len());
    __see(&__o);
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
