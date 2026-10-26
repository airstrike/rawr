fn main() {
    let mut p = (Some(vec![1]), 2);
    if let (Some(ref mut v), _) = p {
        p.1 = 3;
        v.push(4);
    }
    let __b = &mut p;
    println!("{:?}", p);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
