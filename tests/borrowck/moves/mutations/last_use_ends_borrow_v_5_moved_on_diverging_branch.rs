fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    if __c() {
        __take(v);
        panic!();
    }
    v.push(3);
    let m = &mut v;
    m.push(4);
    v.push(5);
    println!("{n} {:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
