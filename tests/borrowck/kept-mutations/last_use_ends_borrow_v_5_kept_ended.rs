fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let __k = __keep(&v);
    __see(__k);
    v.push(3);
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
