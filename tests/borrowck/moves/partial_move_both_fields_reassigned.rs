struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let a = s.a;
    let b = s.b;
    s.a = b;
    s.b = a;
    let t = s;
    println!("{} {}", t.a, t.b);
}
