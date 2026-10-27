struct D {
    a: String,
}

impl Drop for D {
    fn drop(&mut self) {}
}

fn main() {
    let d = D { a: String::from("ab") };
    let D { ref a } = d;
    println!("{}", a);
}
