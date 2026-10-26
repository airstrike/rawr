fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    println!("{r:?}");
    o.take();
    let mut __o = Some(&o);
    __see(&__o);
    __o = None;
    println!("{o:?}");
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
