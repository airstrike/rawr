fn shorter<'a, 'b: 'a>(a: &'a str, b: &'b str) -> &'a str {
    if a.len() < b.len() { a } else { b }
}

fn main() {
    let x = String::from("aaaa");
    let mut y = String::from("bb");
    let s = shorter(&x, &y);
    let __h = __Hold(&y);
    y.push_str("!");
    __see(&__h);
    println!("{s}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
