fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    match v {
        __v => __take(__v),
    }
    v.push(2);
    println!("{}", s.len());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
