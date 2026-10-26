fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &mut *r;
    r.push(3);
    s.push(2);
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        println!("{:?}", v);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
