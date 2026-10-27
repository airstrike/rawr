struct S {
    a: String,
    b: String,
}

fn f(s: &S) -> &String {
    match *s {
        S { ref a, .. } => a,
    }
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("c") };
    println!("{} {}", f(&s), s.b);
}
