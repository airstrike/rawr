fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    r = &b;
    a.push(3);
    for __i in 0..2 {
        let __m = a;
        a = __m;
    }
    println!("{} {:?}", r.len(), a);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
