fn main() {
    let mut v = vec![1, 2];
    for x in &v {
        if *x > 5 {
            v.push(1);
        }
    }
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
