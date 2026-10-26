fn pick<'a, 'b>(a: &'a str, _b: &'b str) -> &'a str {
    a
}

fn main() {
    let mut x = String::from("kept");
    let y = String::from("free");
    let p = pick(&x, &y);
    let mut __o = Some(&x);
    __see(&__o);
    __o = None;
    x.push_str("!");
    __see(&__o);
    println!("{p} {y}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
