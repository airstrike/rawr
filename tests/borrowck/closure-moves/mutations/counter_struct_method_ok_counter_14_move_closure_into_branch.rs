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
    if __c() {
        let __m = move || __see(&counter);
        __m();
    }
    println!("{}", counter.n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
