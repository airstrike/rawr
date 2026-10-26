fn pick<'a, 'b>(a: &'a str, _b: &'b str) -> &'a str {
    a
}

fn main() {
    let mut x = String::from("kept");
    let y = String::from("free");
    let p = pick(&x, &y);
    let __t = if __c() { &__id(&x) } else { &__id(&x) };
    x.push_str("!");
    __see(__t);
    println!("{p} {y}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
