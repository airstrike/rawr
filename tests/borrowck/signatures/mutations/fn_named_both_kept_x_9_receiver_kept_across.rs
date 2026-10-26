fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let mut x = String::from("long one");
    let y = String::from("short");
    let l = longest(&x, &y);
    let __t = vec![&x].first();
    x.push_str("!");
    __see(&__t);
    println!("{l}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
