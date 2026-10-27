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
    let c = move || s.a.len() + s.n.len();
    println!("{} {}", c(), s.b.len() + s.n.len());
}
