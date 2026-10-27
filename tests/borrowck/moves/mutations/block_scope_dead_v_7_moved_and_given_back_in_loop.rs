fn main() {
    let mut v = vec![1];
    {
        let r = &mut v;
        r.push(2);
    }
    for __i in 0..2 {
        let __m = v;
        v = __m;
    }
    v.push(3);
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
