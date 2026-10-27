fn main() {
    let mut v = vec![1];
    let mut keep = &v;
    let mut n = 0;
    while n < 3 {
        v.push(n);
        keep = &v;
        n += 1;
    }
    for __i in 0..2 {
        if __c() {
            __take(keep);
        }
    }
    println!("{:?}", keep);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
