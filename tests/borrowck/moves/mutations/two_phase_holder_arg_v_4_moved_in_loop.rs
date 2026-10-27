fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    for __i in 0..2 {
        if __c() {
            __take(v);
        }
    }
    r.push(v.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
