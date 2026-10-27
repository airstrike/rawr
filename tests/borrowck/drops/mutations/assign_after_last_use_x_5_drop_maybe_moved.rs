fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let __d = __See(&x);
    if __c() {
        drop(__d);
    }
    x = 2;
    println!("{}", x + y);
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
