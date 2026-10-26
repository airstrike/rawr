struct Counter {
    n: i32,
}

impl Counter {
    fn get(&self) -> i32 {
        self.n
    }
}

fn main() {
    let mut c = Counter { n: 1 };
    let n = c.get();
    __see(&vec![&c].first());
    c.n += n;
    println!("{}", c.n);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
