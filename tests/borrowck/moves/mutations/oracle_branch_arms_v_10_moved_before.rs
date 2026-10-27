fn main() {
    let mut v = vec![1];
    let c = v.len() == 1;
    let r = &mut v;
    if c {
        v.push(1)
    } else {
        r.push(2)
    }
    __take(v);
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
