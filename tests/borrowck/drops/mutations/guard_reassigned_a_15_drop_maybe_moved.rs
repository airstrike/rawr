struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn main() {
    let mut a = vec![];
    let mut b = vec![];
    let mut g = Guard(&a);
    println!("{}", g.0.len());
    g = Guard(&b);
    let __d = __See(&a);
    if __c() {
        drop(__d);
    }
    a.push(2);
    println!("{} {}", a.len(), g.0.len());
    b.push(1);
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
