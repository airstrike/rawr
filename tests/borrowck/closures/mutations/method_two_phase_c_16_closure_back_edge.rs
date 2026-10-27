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
    c.add(c.len());
    let r = &c;
    let mut __f = || __touch(&mut c);
    for __i in 0..2 {
        __f();
        c.add(r.len());
    }
    println!("{:?}", c.v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
