struct S {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn main() {
    let s = S { a: vec![1], b: vec![2] };
    let r = &s.a;
    let c = move || s.a.len();
    println!("{} {:?}", c(), r);
}
