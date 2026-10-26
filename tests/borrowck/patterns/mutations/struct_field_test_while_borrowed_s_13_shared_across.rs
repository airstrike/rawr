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
    let __s = &s;
    println!("{} {}", n, s.a);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
