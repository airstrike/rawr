struct S {
    a: String,
    b: String,
}

impl Drop for S {
    fn drop(&mut self) {}
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let c = move || s.a.len();
    println!("{} {}", c(), s.b);
}
