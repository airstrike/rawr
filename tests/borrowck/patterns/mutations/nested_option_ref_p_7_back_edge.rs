fn main() {
    let mut p = (Some(vec![1]), 2);
    if let (Some(ref mut v), _) = p {
        p.1 = 3;
        v.push(4);
    }
    let __b = &mut p;
    for __i in 0..2 {
        __touch(__b);
        println!("{:?}", p);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
