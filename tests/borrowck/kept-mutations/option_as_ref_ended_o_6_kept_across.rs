fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    println!("{r:?}");
    o.take();
    let __k = __keep(&o);
    println!("{o:?}");
    __see(__k);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
