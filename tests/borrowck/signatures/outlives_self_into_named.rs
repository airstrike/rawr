struct S {
    name: String,
}

impl S {
    fn pick<'a>(&self, _other: &'a str) -> &'a str {
        self.name.as_str()
    }
}

fn main() {
    let s = S { name: String::from("n") };
    println!("{}", s.pick("x"));
}
