fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __h = __Hold(&s);
    s.push('b');
    __see(&__h);
    println!("{r}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
