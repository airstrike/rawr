fn main() {
    let mut v = vec![1, 2];
    let __d = __See(&v);
    drop(__d);
    v[v.len() - 1] = 5;
    println!("{:?}", v);
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
