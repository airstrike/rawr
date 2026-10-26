fn main() {
    let mut v = vec![1, 2];
    for x in v.iter() {
        if *x > 5 {
            v.push(*x);
        }
    }
    let mut __o = Some(&v);
    __see(&__o);
    __o = None;
    println!("{v:?}");
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
