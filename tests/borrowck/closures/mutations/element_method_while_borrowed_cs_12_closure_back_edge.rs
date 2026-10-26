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
    let mut __f = || __touch(&mut cs);
    for __i in 0..2 {
        __f();
        cs[0].add(1);
    }
    println!("{}", r.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
