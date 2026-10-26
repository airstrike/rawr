fn main() {
    let mut v = vec![1, 2];
    let w = vec![3];
    let s = &w;
    let c = || s;
    let r = c();
    v.push(3);
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        println!("{:?} {:?}", r, v);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
