fn main() {
    let mut a = 1;
    let b = 2;
    let mut o = Some(&a);
    println!("{o:?}");
    let __t = vec![&o].to_vec();
    o = Some(&b);
    __see(&__t);
    a += 1;
    println!("{o:?} {a}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
