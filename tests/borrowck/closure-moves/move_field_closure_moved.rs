struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let c = move || s.a.len();
    let d = c;
    println!("{} {} {}", d(), c(), s.b);
}
