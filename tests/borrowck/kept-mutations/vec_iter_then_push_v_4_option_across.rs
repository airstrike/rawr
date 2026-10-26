fn main() {
    let mut v = vec![1, 2];
    let it = v.iter();
    let __o = Some(&v);
    v.push(3);
    __see(&__o);
    println!("{}", it.count());
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
