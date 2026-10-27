fn main() {
    let mut v = vec![1, 2];
    for __i in 0..2 {
        if __c() {
            __take(v);
        }
    }
    v[v.len() - 1] = 5;
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
