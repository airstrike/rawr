struct S {
    v: Vec<i32>,
    n: usize,
}

fn main() {
    let v = vec![1, 2];
    let s = S { v, n: v.len() };
    println!("{:?} {}", s.v, s.n);
}
