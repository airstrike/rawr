fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    match v {
        __v => __take(__v),
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
