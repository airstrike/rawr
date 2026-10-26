struct S {
    a: i64,
    v: Vec<i64>,
}
fn main() {
    let s = S { a: 1, v: vec![2] };
    let r = &s.a;
    let w = match s {
        S { v, .. } => v,
    };
    println!("{:?} {:?}", r, w);
}
