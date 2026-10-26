fn main() {
    let mut v = vec![1, 2, 3];
    let last = v.last_mut().unwrap();
    *last = 9;
    let __v = vec![&v];
    println!("{} {:?}", v.len(), v);
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
