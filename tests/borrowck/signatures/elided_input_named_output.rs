fn f<'a>(x: &'a str, y: &str) -> &'a str {
    if x.is_empty() { y } else { x }
}

fn main() {
    println!("{}", f("", "b"));
}
