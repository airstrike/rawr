fn main() {
    let mut v = vec![1, 2];
    let __v = vec![&v];
    v.push(v.len());
    __see(&__v);
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
