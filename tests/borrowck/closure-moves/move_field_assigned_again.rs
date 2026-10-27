struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let c = move || s.a.len();
    s.a = String::from("x");
    let t = s;
    println!("{} {} {}", c(), t.a, t.b);
}
