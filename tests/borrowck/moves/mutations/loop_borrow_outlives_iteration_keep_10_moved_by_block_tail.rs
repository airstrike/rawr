fn main() {
    let mut v = vec![1];
    let mut keep = &v;
    let mut n = 0;
    while n < 3 {
        v.push(n);
        keep = &v;
        n += 1;
    }
    let __m = { keep };
    println!("{:?}", keep);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
