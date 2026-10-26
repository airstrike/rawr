fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __k = __keep(&s);
    s.push('b');
    __see(__k);
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
