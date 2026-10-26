fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __k = __keep(&v);
    v.push(2);
    if __c() {
        __see(__k);
    }
    println!("{}", s.len());
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
