fn pick<'a, 'b>(a: &'a str, _b: &'b str) -> &'a str {
    a
}

fn main() {
    let mut x = String::from("kept");
    let y = String::from("free");
    let p = pick(&x, &y);
    let __k = __keep(&x);
    x.push_str("!");
    if __c() {
        __see(__k);
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
