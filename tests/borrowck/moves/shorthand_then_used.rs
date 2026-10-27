struct S {
    v: Vec<i32>,
}

fn main() {
    let v = vec![1, 2];
    let s = S { v };
    println!("{:?} {:?}", s.v, v);
}
