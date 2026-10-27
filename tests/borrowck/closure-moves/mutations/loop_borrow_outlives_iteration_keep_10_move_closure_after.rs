fn main() {
    let mut v = vec![1];
    let mut keep = &v;
    let mut n = 0;
    while n < 3 {
        v.push(n);
        keep = &v;
        n += 1;
    }
    println!("{:?}", keep);
    let __m = move || __see(&keep);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
