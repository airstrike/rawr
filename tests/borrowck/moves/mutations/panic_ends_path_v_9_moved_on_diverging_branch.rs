fn main() {
    let mut v = vec![1];
    let r = &mut v;
    if v.is_empty() {
        v.push(1);
        panic!("empty");
    }
    r.push(2);
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
