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
    c();
    let mut __f = || __touch(&mut counter);
    println!("{}", counter.n);
    __f();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
