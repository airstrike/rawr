fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    for __i in 0..2 {
        if __c() {
            __take(v);
        }
    }
    v.push(2);
    println!("{}", s.len());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
