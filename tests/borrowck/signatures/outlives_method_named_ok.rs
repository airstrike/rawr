struct S {
    name: String,
}

impl S {
    fn pick<'a>(&'a self, other: &'a str) -> &'a str {
        if self.name.is_empty() { other } else { self.name.as_str() }
    }

    fn trimmed(&self) -> &str {
        self.name.trim()
    }
}

fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let s = S { name: String::from(" n ") };
    println!("{} {} {}", s.pick("x"), s.trimmed(), longest("ab", "c"));
}
