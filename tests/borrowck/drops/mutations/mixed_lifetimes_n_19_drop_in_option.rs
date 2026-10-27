struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

struct Both<'a, 'b> {
    g: Guard<'a>,
    r: &'b i32,
}

fn main() {
    let v = vec![];
    let mut n = 1;
    let b = Both { g: Guard(&v), r: &n };
    n += 1;
    let __o = Some(__See(&n));
    println!("{}", n);
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
