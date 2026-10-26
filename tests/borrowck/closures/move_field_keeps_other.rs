struct S {
    a: Vec<i32>,
    b: Vec<i32>,
}
fn main() {
    let s = S { a: vec![1], b: vec![2] };
    let r = &s.b;
    let c = move || s.a.len();
    println!("{} {}", r.len(), c());
}
