fn main() {
    let mut v = vec![1, 2];
    for x in &v {
        if *x > 5 {
            v.push(1);
        }
    }
    if __c() {
        __take(v);
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
