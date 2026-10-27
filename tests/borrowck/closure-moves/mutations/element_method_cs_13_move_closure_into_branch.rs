struct C {
    v: Vec<i64>,
}
impl C {
    fn add(&mut self, x: usize) {
        self.v.push(x as i64);
    }
}
fn main() {
    let mut cs = vec![C { v: vec![] }];
    let n = cs.len();
    cs[0].add(n);
    if __c() {
        let __m = move || __see(&cs);
        __m();
    }
    println!("{:?}", cs[0].v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
