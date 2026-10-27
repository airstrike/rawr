fn main() {
    let mut v = vec![1, 2];
    let k = v.len();
    let r = &mut v;
    match k {
        1 => v.push(1),
        _ => r.push(2),
    }
    for __i in 0..2 {
        if __c() {
            __take(v);
        }
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
