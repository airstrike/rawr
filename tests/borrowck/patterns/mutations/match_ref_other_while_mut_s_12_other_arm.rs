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
    let __b = &mut s;
    if __c() {
        __touch(__b);
    } else {
        println!("{:?}", s.v);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
