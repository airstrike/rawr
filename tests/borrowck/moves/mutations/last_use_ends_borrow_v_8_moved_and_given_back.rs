fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    v.push(3);
    let m = &mut v;
    m.push(4);
    let __m = v;
    v = __m;
    v.push(5);
    println!("{n} {:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
