fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __v = vec![&v];
    v.push(r.len());
    __see(&__v);
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
