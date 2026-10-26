fn main() {
    let mut a = 1;
    let b = 2;
    let mut o = Some(&a);
    println!("{o:?}");
    o = Some(&b);
    a += 1;
    let __o = Some(&a);
    println!("{o:?} {a}");
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
