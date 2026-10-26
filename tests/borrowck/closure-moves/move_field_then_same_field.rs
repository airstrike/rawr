struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let c = move || s.a.len();
    let a = s.a;
    println!("{} {} {}", c(), a, s.b);
}
