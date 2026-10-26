fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    let mut __o = Some(&r);
    __see(&__o);
    __o = None;
    println!("{}", r.len());
    __see(&__o);
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
