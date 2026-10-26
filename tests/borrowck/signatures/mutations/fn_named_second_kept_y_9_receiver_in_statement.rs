fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let x = String::from("long one");
    let mut y = String::from("short");
    let l = longest(&x, &y);
    __see(&vec![&y].first());
    y.push_str("!");
    println!("{l}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
