fn main() {
    let mut v = vec![1];
    let mut keep = &v;
    let mut n = 0;
    while n < 3 {
        v.push(n);
        keep = &v;
        n += 1;
    }
    let __d = __See(&keep);
    if __c() {
        drop(__d);
    }
    println!("{:?}", keep);
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
