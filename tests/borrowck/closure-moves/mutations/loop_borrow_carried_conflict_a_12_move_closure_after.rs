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
    println!("{:?}", a);
    let __m = move || __see(&a);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
