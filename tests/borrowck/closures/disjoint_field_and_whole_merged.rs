struct S {
    a: Vec<i32>,
    b: Vec<i32>,
}
fn main() {
    let mut s = S { a: vec![1], b: vec![2] };
    let c = || {
        let n = s.a.len();
        n + s.b.len()
    };
    s.b.push(3);
    println!("{}", c());
}
