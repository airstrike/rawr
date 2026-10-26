fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    let __h = __Hold(&o);
    o.take();
    __see(&__h);
    println!("{r:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
