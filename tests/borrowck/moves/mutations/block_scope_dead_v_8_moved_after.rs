fn main() {
    let mut v = vec![1];
    {
        let r = &mut v;
        r.push(2);
    }
    v.push(3);
    println!("{:?}", v);
    __take(v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
