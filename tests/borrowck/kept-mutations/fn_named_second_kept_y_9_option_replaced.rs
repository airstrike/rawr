fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let x = String::from("long one");
    let mut y = String::from("short");
    let l = longest(&x, &y);
    let mut __o = Some(&y);
    __see(&__o);
    __o = None;
    y.push_str("!");
    __see(&__o);
    println!("{l}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
