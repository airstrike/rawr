struct Counter {
    n: i32,
}
impl Counter {
    fn bump(&mut self) {
        self.n += 1;
    }
}
fn main() {
    let mut counter = Counter { n: 0 };
    let mut c = || counter.bump();
    c();
    println!("{}", counter.n);
    let mut __f = || __touch(&mut c);
    let mut __g = __f;
    __g();
    __f();
    c();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
