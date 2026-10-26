fn first<'c>(a: &'c str, _b: &str) -> &'c str {
    a
}

fn pick<'a, 'b>(x: &'a str, y: &'b str) -> &'a str {
    first(y, x)
}

fn main() {
    println!("{}", pick("a", "b"));
}
