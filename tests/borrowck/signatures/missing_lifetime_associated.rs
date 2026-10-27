struct S;

impl S {
    fn pick(a: &str, b: &str) -> &str {
        if a.len() > b.len() { a } else { b }
    }
}

fn main() {
    println!("{}", S::pick("a", "bc"));
}
