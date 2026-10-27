fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    let ref __v = v;
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
