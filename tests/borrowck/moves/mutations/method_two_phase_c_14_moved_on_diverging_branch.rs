struct C {
    v: Vec<i64>,
}
impl C {
    fn add(&mut self, x: usize) {
        self.v.push(x as i64);
    }
    fn len(&self) -> usize {
        self.v.len()
    }
}
fn main() {
    let mut c = C { v: vec![] };
    if __c() {
        __take(c);
        panic!();
    }
    c.add(c.len());
    let r = &c;
    c.add(r.len());
    println!("{:?}", c.v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
