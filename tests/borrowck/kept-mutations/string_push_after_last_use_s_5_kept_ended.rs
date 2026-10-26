fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __k = __keep(&s);
    __see(__k);
    s.push('b');
    println!("{n} {s}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
