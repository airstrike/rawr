fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let mut __f = || __touch(&mut v);
    v.push(3);
    if __c() {
        __f();
    }
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
