struct S {
    a: String,
}

fn take(s: &S) -> String {
    s.a
}

fn main() {
    let s = S { a: String::from("ab") };
    println!("{}", take(&s));
}
