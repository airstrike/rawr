fn main() {
    let mut p = (Some(vec![1]), 2);
    if let (Some(ref mut v), _) = p {
        let n = p.0.is_some();
        v.push(4);
        println!("{}", n);
    }
    for __i in 0..2 {
        let __b = &mut p;
        __touch(__b);
        println!("{:?}", p);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
