struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn keep(g: Guard<'_>, v: &mut Vec<i32>) {
    v.push(1);
    println!("{}", g.0.len());
}

fn main() {
    let a = vec![];
    let mut b = vec![];
    let __d = __See(&b);
    if __c() {
        drop(__d);
    }
    keep(Guard(&a), &mut b);
    println!("{} {}", a.len(), b.len());
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
