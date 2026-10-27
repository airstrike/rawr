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
    let r = &cs;
    for __i in 0..2 {
        if __c() {
            __take(cs);
        }
    }
    cs[0].add(1);
    println!("{}", r.len());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
