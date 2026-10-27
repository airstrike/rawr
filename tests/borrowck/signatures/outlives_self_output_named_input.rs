struct S {
    name: String,
}

impl S {
    fn pick<'b>(&self, other: &'b str) -> &str {
        if self.name.is_empty() { other } else { self.name.as_str() }
    }
}

fn main() {
    let s = S { name: String::new() };
    println!("{}", s.pick("x"));
}
