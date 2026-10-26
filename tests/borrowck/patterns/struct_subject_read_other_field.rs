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
    println!("{} {} {}", n, s.a, s.b);
}
