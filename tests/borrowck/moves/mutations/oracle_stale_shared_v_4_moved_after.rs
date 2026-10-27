fn main() {
    let mut v = vec![1];
    let r = &v;
    v.push(1);
    __take(v);
    println!("{r:?}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
