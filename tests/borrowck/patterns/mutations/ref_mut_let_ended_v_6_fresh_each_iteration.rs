fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    a.push(2);
    v.push(1);
    for __i in 0..2 {
        let __b = &mut v;
        __touch(__b);
        println!("{:?}", v);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
