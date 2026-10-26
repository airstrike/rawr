fn main() {
    let mut v = vec![1, 2];
    let n = v.len();
    v.push(3);
    let __v = vec![&v];
    println!("{n} {v:?}");
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
