fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let ref __v = v;
    v.push(2);
    println!("{}", s.len());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
