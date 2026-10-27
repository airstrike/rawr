fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    {
        let __d = __See(&r);
    }
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
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
