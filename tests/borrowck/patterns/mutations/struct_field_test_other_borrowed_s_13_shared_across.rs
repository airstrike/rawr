struct S {
    a: i64,
    v: Vec<i64>,
}
fn main() {
    let mut s = S { a: 1, v: vec![] };
    let r = &mut s.v;
    let n = match s {
        S { a: 1, .. } => 10,
        _ => 20,
    };
    r.push(1);
    let __s = &s;
    println!("{} {:?}", n, s.v);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
