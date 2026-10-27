fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut prev = &mut a;
    let mut n = 0;
    while n < 2 {
        prev.push(n);
        let r = &mut b;
        r.push(n);
        prev = r;
        n += 1;
    }
    __see(&vec![&b].first());
    b.push(9);
    println!("{:?} {:?}", a, b);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
