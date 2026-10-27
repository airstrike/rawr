struct S {
    n: i32,
}

fn main() {
    let n = 3;
    let s = S { n };
    println!("{} {}", s.n, n);
}
