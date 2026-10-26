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
    let __k = __keep(&c);
    c.add(c.len());
    __see(__k);
    let r = &c;
    c.add(r.len());
    println!("{:?}", c.v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
