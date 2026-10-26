struct S {
    a: i64,
}
fn main() {
    let mut s = S { a: 1 };
    let m = &mut s;
    match s {
        _ => println!("any"),
    }
    m.a += 1;
    for __i in 0..2 {
        let __b = &mut s;
        __touch(__b);
        println!("{}", s.a);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
