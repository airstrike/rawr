fn shorter<'a, 'b: 'a>(a: &'a str, b: &'b str) -> &'a str {
    if a.len() < b.len() { a } else { b }
}

fn main() {
    let x = String::from("aaaa");
    let mut y = String::from("bb");
    let s = shorter(&x, &y);
    y.push_str("!");
    println!("{s}");
}
