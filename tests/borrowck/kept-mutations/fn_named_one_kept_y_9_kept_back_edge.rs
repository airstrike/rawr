fn pick<'a, 'b>(a: &'a str, _b: &'b str) -> &'a str {
    a
}

fn main() {
    let x = String::from("kept");
    let mut y = String::from("free");
    let p = pick(&x, &y);
    let __k = __keep(&y);
    for __i in 0..2 {
        __see(__k);
        y.push_str("!");
    }
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
