fn pick<'a, 'b>(x: &'a str, y: &'b str) -> &'a str {
    let r = y;
    if x.is_empty() {
        return r;
    }
    x
}

fn main() {
    println!("{}", pick("", "b"));
}
