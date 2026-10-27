fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    for __i in 0..2 {
        let __m = r;
        r = __m;
    }
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
