fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = a.len() > 0;
    let r = if c { &a } else { &b };
    let __o = Some(__See(&b));
    b.push(3);
    println!("{}", r.len());
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
