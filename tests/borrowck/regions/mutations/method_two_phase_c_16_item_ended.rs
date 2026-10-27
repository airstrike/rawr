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
    let __v = vec![&c];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    c.add(r.len());
    println!("{:?}", c.v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
