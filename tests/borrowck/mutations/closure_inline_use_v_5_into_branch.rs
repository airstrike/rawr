fn main() {
    let mut v = vec![1, 2, 3];
    let k = 2;
    let doubled: Vec<i64> = v.iter().map(|x| x * k).collect();
    let __b = &mut v;
    v.push(doubled.len() as i64);
    if __c() {
        __touch(__b);
    }
    println!("{:?} {:?}", v, doubled);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
