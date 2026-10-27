struct S {
    a: String,
    b: String,
}

impl S {
    fn len(&self) -> usize {
        self.b.len()
    }
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    let a = s.a;
    println!("{} {}", a, s.len());
}
