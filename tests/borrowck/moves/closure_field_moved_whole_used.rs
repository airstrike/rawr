struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let c = || s.a;
    let t = s;
    println!("{} {}", t.b, c());
}
