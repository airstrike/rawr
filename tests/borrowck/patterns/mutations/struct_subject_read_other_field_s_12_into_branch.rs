struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    let n = match s {
        S { b, .. } => b,
    };
    *r += 1;
    let __b = &mut s;
    println!("{} {} {}", n, s.a, s.b);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
