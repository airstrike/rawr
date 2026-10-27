struct D {
    a: String,
}

impl Drop for D {
    fn drop(&mut self) {}
}

fn main() {
    match (D { a: String::from("ab") }) {
        D { a } => println!("{}", a),
    }
}
