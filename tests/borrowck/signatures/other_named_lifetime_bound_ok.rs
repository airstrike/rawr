fn f<'a, 'b: 'a>(x: &'a str, y: &'b str) -> &'a str {
    if x.is_empty() { y } else { x }
}

fn main() {
    println!("{}", f("", "b"));
}
