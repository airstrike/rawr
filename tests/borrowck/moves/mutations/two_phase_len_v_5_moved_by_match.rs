fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    match v {
        __v => __take(__v),
    }
    v.push(r.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
