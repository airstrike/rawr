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
    b.push(9);
    let __f = || __see(&a);
    let __g = __f;
    __f();
    println!("{:?} {:?}", a, b);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
