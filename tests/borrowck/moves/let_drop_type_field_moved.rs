struct D {
    a: String,
}

impl Drop for D {
    fn drop(&mut self) {}
}

fn main() {
    let D { a } = D { a: String::from("ab") };
    println!("{}", a);
}
