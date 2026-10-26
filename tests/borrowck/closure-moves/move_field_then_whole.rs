struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let c = move || s.a.len();
    let t = s;
    println!("{} {}", c(), t.b);
}
