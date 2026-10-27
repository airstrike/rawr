fn main() {
    let mut v = vec![1];
    let mut n = 0;
    while n < 3 {
        let r = &mut v;
        r.push(n);
        n += v.len() as i64;
    }
    match n {
        __v => __take(__v),
    }
    println!("{n} {:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
