fn main() {
    let mut v = vec![1];
    let r = &mut v;
    if v.is_empty() {
        v.push(1);
        panic!("empty");
    }
    r.push(2);
    match v {
        __v => __take(__v),
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
