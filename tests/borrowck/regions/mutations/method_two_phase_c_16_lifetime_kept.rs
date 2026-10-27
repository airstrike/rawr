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
    let __u = 0u8;
    let __p = __Two(&c, &__u);
    let __k = __p.0;
    c.add(r.len());
    __see(&__k);
    println!("{:?}", c.v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
