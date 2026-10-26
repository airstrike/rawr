struct S {
    a: String,
    b: String,
    n: String,
}

fn main() {
    let s = S {
        a: String::from("ab"),
        b: String::from("cde"),
        n: String::from("f"),
    };
    let c = move || s.a.len() + s.b.len();
    println!("{} {}", c(), s.n);
}
