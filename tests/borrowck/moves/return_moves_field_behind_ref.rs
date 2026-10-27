struct S {
    a: String,
    b: String,
}

fn f(s: &S) -> String {
    return s.a;
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("c") };
    println!("{}", f(&s));
}
