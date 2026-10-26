fn main() {
    let mut v = vec![1, 2];
    let w = v.to_vec();
    let __v = vec![&v];
    v.push(3);
    __see(&__v);
    println!("{w:?} {v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
