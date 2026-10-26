fn main() {
    let mut v = vec![10, 20];
    let x = v.get(1).unwrap();
    let mut __o = Some(&v);
    __see(&__o);
    __o = None;
    v.clear();
    __see(&__o);
    println!("{x}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
