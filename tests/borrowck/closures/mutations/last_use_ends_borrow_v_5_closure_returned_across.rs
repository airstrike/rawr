fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let __h = || &v;
    let __k = __h();
    v.push(3);
    __see(__k);
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
