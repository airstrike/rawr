fn shorter<'a, 'b>(a: &'a str, b: &'b str) -> &'a str
where
    'b: 'a,
{
    if a.len() < b.len() { a } else { b }
}

fn main() {
    let x = String::from("aaaa");
    let mut y = String::from("bb");
    let s = shorter(&x, &y);
    let __t = &__id(&y);
    y.push_str("!");
    __see(__t);
    println!("{s}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
