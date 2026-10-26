fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        v.push(3);
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
