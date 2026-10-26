fn main() {
    let text = String::from(" a ");
    let other = String::from("b");
    let mut s: &str = &text;
    let t = s.trim();
    let __o = Some(&s);
    s = &other;
    __see(&__o);
    println!("{t} {s}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
