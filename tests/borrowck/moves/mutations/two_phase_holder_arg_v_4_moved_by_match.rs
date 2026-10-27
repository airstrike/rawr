fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    match v {
        __v => __take(__v),
    }
    r.push(v.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
