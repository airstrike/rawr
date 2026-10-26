fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    let __v = vec![&v];
    println!("{:?}", v);
    __see(&__v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
