fn main() {
    let mut a = 1;
    let b = 2;
    let mut o = Some(&a);
    println!("{o:?}");
    let __o = Some(&o);
    o = Some(&b);
    __see(&__o);
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
