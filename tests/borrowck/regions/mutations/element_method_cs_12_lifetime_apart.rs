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
    let __u = 0u8;
    let __p = __Two(&__u, &cs);
    let __k = __p.0;
    cs[0].add(n);
    __see(&__k);
    println!("{:?}", cs[0].v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
