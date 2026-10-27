fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __o = Some(__See(&x));
    println!("{}", *r + x);
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
