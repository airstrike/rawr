fn main() {
    let mut v = vec![1, 2];
    let mut total = 0;
    for x in &v {
        total += *x;
    }
    match total {
        __v => __take(__v),
    }
    v.push(total);
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
