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
    let __v = vec![&cs];
    let __k = __v.iter().copied().next().unwrap();
    cs[0].add(1);
    __see(&__k);
    println!("{}", r.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
