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
    c.n += n;
    let __t = vec![&c].to_vec();
    println!("{}", c.n);
    __see(&__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
