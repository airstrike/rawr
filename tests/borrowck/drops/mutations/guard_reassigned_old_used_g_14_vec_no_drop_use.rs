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
    a.push(2);
    let __v = vec![&g];
    g = Guard(&b);
    println!("{}", g.0.len());
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
