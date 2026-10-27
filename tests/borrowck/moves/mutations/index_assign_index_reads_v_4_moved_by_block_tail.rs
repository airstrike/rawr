fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    let __m = { v };
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
