fn f(x: &str) -> &'static str {
    x
}

fn main() {
    println!("{}", f("b"));
}
