fn pick<'a, 'b>(a: &'a str, _b: &'b str) -> &'a str {
    a
}

fn main() {
    let x = String::from("kept");
    let mut y = String::from("free");
    let p = pick(&x, &y);
    y.push_str("!");
    let mut __o = Some(&y);
    __see(&__o);
    __o = None;
    println!("{p} {y}");
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
