struct S {
    v: Vec<i32>,
    n: usize,
}

fn main() {
    let v = vec![1, 2];
    let s = S { n: v.len(), v };
    println!("{:?} {}", s.v, s.n);
}
