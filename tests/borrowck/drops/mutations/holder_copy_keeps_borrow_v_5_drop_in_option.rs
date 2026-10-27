fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __o = Some(__See(&v));
    v.push(2);
    println!("{}", s.len());
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
