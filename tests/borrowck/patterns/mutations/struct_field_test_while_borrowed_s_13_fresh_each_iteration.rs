struct S {
    a: i64,
    v: Vec<i64>,
}
fn main() {
    let mut s = S { a: 1, v: vec![] };
    let r = &mut s.a;
    let n = match s {
        S { a: 1, .. } => 10,
        _ => 20,
    };
    *r += 1;
    for __i in 0..2 {
        let __b = &mut s;
        __touch(__b);
        println!("{} {}", n, s.a);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
