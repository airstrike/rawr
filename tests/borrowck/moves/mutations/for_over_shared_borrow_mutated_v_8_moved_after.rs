fn main() {
    let mut v = vec![1, 2];
    for x in &v {
        if *x > 5 {
            v.push(1);
        }
    }
    println!("{:?}", v);
    __take(v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
