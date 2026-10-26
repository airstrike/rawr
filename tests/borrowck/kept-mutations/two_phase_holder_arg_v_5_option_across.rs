fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    let __o = Some(&v);
    println!("{:?}", v);
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
