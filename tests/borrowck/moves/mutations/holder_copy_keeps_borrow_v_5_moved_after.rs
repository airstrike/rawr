fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    v.push(2);
    __take(v);
    println!("{}", s.len());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
