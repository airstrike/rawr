fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let mut x = String::from("long one");
    let y = String::from("short");
    let l = longest(&x, &y);
    let mut __o = Some(&x);
    __see(&__o);
    __o = None;
    x.push_str("!");
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
