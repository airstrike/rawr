fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref mut a, ref b) = t;
    a.push(b.len() as i64);
    let __s = &t;
    println!("{:?}", t);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
