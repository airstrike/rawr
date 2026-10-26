struct S {
    a: i64,
    v: Vec<i64>,
}
fn main() {
    let mut s = S { a: 1, v: vec![2] };
    let r = &mut s.v;
    match s {
        S { ref a, .. } => println!("{:?}", a),
    }
    r.push(3);
    let __s = &s;
    println!("{:?}", s.v);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
