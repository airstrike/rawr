fn main() {
    let mut v = vec![1];
    let mut n = 0;
    while n < 3 {
        let r = &mut v;
        r.push(n);
        n += v.len() as i64;
    }
    let __m = v;
    v = __m;
    println!("{n} {:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
