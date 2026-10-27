fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    println!("{:?}", v);
    __take(v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
