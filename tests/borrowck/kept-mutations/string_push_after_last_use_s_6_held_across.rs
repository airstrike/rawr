fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    s.push('b');
    let __h = __Hold(&s);
    println!("{n} {s}");
    __see(&__h);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
