fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __d = __See(&s);
    if __c() {
        drop(__d);
    }
    s.push('b');
    println!("{n} {s}");
}

fn __c() -> bool {
    false
}
fn __eat<T>(_: T) {}
struct __See<'a, T>(&'a T);
impl<T> Drop for __See<'_, T> {
    fn drop(&mut self) {}
}
struct __Hold<'a, T> {
    s: __See<'a, T>,
}
