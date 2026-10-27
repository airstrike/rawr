struct S {
    n: i32,
    b: String,
}

fn main() {
    let s = S { n: 4, b: String::from("cde") };
    let c = move || s.n + 1;
    let t = s;
    println!("{} {} {}", c(), t.n, t.b);
}
