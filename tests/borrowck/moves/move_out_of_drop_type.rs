struct S {
    a: String,
}

impl Drop for S {
    fn drop(&mut self) {}
}

fn main() {
    let s = S { a: String::from("ab") };
    let a = s.a;
    println!("{}", a);
}
