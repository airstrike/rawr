fn main() {
    let mut v = vec![1, 2];
    __take(v);
    v[v.len() - 1] = 5;
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
