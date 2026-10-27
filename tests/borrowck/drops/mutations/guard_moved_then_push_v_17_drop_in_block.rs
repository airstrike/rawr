struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn take(g: Guard<'_>) {
    println!("took {}", g.0.len());
}

fn main() {
    let mut v = vec![];
    let g = Guard(&v);
    take(g);
    {
        let __d = __See(&v);
    }
    v.push(1);
    println!("{}", v.len());
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
