fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let __m = move || __see(&v);
    v.push(3);
    __m();
    let m = &mut v;
    m.push(4);
    v.push(5);
    println!("{n} {:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
