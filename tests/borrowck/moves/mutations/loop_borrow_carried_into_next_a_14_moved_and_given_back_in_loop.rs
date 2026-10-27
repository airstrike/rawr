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
    for __i in 0..2 {
        let __m = a;
        a = __m;
    }
    println!("{:?} {:?}", a, b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
