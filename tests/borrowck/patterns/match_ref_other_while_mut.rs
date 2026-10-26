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
    println!("{:?}", s.v);
}
