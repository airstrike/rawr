fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut prev = &mut a;
    let mut n = 0;
    while n < 2 {
        b.push(n);
        prev.push(n);
        prev = &mut b;
        n += 1;
    }
    __take(a);
    println!("{:?}", a);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
