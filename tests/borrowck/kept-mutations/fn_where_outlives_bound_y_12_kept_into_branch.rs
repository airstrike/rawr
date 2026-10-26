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
    y.push_str("!");
    if __c() {
        __see(__k);
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
