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
    let __b = &mut s;
    println!("{}", s.a);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
