fn main() {
    let mut v = vec![1, 2];
    let mut total = 0;
    for x in &v {
        total += *x;
    }
    let __o = Some(&v);
    v.push(total);
    __see(&__o);
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
