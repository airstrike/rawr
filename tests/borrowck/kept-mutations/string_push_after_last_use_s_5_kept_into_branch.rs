fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __k = __keep(&s);
    s.push('b');
    if __c() {
        __see(__k);
    }
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
