struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let c = || s.a.len();
    let b = s.b;
    println!("{} {}", c(), b);
}
