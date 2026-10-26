fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let __o = Some(&v);
    v.push(3);
    __see(&__o);
    let m = &mut v;
    m.push(4);
    v.push(5);
    println!("{n} {:?}", v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
