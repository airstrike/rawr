fn main() {
    let mut t = (vec![1], vec![2]);
    let mut c = || t.0.push(3);
    let mut __f = || __touch(&mut t);
    for __i in 0..2 {
        __f();
        t.1.push(4);
    }
    c();
    println!("{:?} {:?}", t.0, t.1);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
