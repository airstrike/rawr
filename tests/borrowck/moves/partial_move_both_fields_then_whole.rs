struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let a = s.a;
    let b = s.b;
    let t = &s;
    println!("{} {}", a, b);
}
