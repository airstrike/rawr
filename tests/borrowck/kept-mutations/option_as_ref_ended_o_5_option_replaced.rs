fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    println!("{r:?}");
    let mut __o = Some(&o);
    __see(&__o);
    __o = None;
    o.take();
    __see(&__o);
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
