struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

struct Show<'a>(&'a Noisy);

impl Drop for Show<'_> {
    fn drop(&mut self) {
        println!("show {}", (self.0).0);
    }
}

impl Show<'_> {
    fn name(&self) -> &'static str {
        (self.0).0
    }
}

fn main() {
    let a = Noisy("a");
    println!("{}", Show(&a).name());
    let mut s = Show(&a);
    let __d = __See(&s);
    drop(__d);
    s = Show(&a);
    println!("{}", s.name());
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
