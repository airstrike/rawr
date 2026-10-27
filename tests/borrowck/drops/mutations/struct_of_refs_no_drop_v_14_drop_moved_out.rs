struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

struct Pair<'a>(&'a Vec<i32>, i32);

fn main() {
    let mut v = vec![];
    let p = Pair(&v, 1);
    let __d = __See(&v);
    __eat(__d);
    v.push(2);
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
