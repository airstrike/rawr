fn main() {
    let mut a = 1;
    let b = 2;
    let mut o = Some(&a);
    let mut __o = Some(&o);
    __see(&__o);
    __o = None;
    println!("{o:?}");
    __see(&__o);
    o = Some(&b);
    a += 1;
    println!("{o:?} {a}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
