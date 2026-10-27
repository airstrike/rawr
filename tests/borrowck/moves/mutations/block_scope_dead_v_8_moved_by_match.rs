fn main() {
    let mut v = vec![1];
    {
        let r = &mut v;
        r.push(2);
    }
    v.push(3);
    match v {
        __v => __take(__v),
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
