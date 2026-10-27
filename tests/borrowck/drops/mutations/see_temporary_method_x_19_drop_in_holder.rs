struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

impl See<'_> {
    fn get(&self) -> i32 {
        *self.0
    }
}

fn main() {
    let mut x = 1;
    let n = See(&x).get();
    x = n + 1;
    let __h = __Hold { s: __See(&x) };
    println!("{}", x);
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
