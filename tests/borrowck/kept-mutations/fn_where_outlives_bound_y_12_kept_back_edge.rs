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
    let __k = __keep(&y);
    for __i in 0..2 {
        __see(__k);
        y.push_str("!");
    }
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
