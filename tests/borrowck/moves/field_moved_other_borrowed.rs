struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let r = &s.b;
    let a = s.a;
    println!("{} {}", r, a);
}
