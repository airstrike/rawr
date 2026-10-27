fn main() {
    let mut v = vec![1];
    {
        let r = &mut v;
        r.push(2);
    }
    v.push(3);
    if __c() {
        __take(v);
        panic!();
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
