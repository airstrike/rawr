fn main() {
    let mut v = vec![1];
    let a = &mut v;
    let b = &mut v;
    b.push(2);
    a.push(3);
    println!("{:?}", v);
    __take(v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
