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
    let mut __o = Some(&cs);
    __see(&__o);
    __o = None;
    println!("{:?}", cs[0].v);
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
