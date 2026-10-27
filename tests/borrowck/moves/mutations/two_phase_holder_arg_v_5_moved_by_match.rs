fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    match v {
        __v => __take(__v),
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
