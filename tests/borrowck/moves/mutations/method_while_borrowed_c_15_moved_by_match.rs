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
    let r = &c;
    match c {
        __v => __take(__v),
    }
    c.add(1);
    println!("{}", r.len());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
