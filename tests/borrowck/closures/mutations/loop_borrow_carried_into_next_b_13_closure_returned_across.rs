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
    let __h = || &b;
    let __k = __h();
    b.push(9);
    __see(__k);
    println!("{:?} {:?}", a, b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
