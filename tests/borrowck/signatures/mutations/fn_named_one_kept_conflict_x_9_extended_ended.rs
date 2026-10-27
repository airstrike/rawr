fn pick<'a, 'b>(a: &'a str, _b: &'b str) -> &'a str {
    a
}

fn main() {
    let mut x = String::from("kept");
    let y = String::from("free");
    let p = pick(&x, &y);
    let __t = &__id(&x);
    __see(__t);
    x.push_str("!");
    println!("{p} {y}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
