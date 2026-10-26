fn main() {
    let mut a = 1;
    let b = 2;
    let mut o = Some(&a);
    __see(&vec![&o].first());
    println!("{o:?}");
    o = Some(&b);
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
