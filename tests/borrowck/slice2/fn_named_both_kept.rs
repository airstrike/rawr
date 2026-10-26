fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let mut x = String::from("long one");
    let y = String::from("short");
    let l = longest(&x, &y);
    x.push_str("!");
    println!("{l}");
}
