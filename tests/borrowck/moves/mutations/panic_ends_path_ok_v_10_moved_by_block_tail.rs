fn main() {
    let mut v = vec![1];
    let empty = v.is_empty();
    let r = &mut v;
    if empty {
        v.push(1);
        panic!("empty");
    }
    r.push(2);
    let __m = { v };
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
