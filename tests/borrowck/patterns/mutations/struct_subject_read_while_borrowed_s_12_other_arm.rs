struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    let n = match s {
        S { a, .. } => a,
    };
    *r += 1;
    let __b = &mut s;
    if __c() {
        __touch(__b);
    } else {
        println!("{} {} {}", n, s.a, s.b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
