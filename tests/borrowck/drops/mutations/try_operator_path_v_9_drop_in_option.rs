fn first(v: &mut Vec<i64>) -> Option<i64> {
    let r = &mut *v;
    let x = r.first().copied()?;
    v.push(x);
    Some(x)
}
fn main() {
    let mut v = vec![4];
    let __o = Some(__See(&v));
    println!("{:?} {:?}", first(&mut v), v);
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
