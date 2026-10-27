struct S {
    name: String,
}

impl S {
    fn name(&self) -> &str {
        &self.name
    }
}

fn make() -> S {
    S { name: String::from("s") }
}

fn main() {
    let n = make().name();
    println!("{n}");
}
