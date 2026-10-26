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
    println!("{} {}", n, s.a);
}
