struct S {
    name: String,
}

impl S {
    fn pick(&self, other: &str) -> &str {
        if other.is_empty() { &self.name } else { &self.name[..1] }
    }
}

fn one(a: &str, n: usize) -> &str {
    &a[..n]
}

fn main() {
    let s = S { name: String::from("name") };
    println!("{} {}", s.pick("x"), one("abc", 2));
}
