fn f(s: &'static str) -> &str {
    s
}

fn main() {
    println!("{}", f("a"));
}
