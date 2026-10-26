fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let mut x = String::from("long one");
    let y = String::from("short");
    let l = longest(&x, &y);
    let __k = __keep(&x);
    for __i in 0..2 {
        __see(__k);
        x.push_str("!");
    }
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
