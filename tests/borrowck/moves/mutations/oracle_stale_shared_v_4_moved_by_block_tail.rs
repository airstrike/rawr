fn main() {
    let mut v = vec![1];
    let r = &v;
    let __m = { v };
    v.push(1);
    println!("{r:?}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
