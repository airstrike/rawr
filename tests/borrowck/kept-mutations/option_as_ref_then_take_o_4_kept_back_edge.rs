fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    let __k = __keep(&o);
    for __i in 0..2 {
        __see(__k);
        o.take();
    }
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
