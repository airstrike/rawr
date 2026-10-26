fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    println!("{r:?}");
    let __h = __Hold(&o);
    o.take();
    __see(&__h);
    println!("{o:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
