struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let c = || s.a;
    let a = &s.a;
    println!("{} {}", a, c());
}
