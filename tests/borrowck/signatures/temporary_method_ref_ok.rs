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
    let n = make().name().len();
    let s = make();
    let m = s.name();
    println!("{n} {m} {}", make().name());
}
