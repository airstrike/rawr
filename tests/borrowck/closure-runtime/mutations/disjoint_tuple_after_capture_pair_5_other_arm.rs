fn main() {
    let mut pair = (vec![1, 2, 3, 4], vec![2]);
    let mut push = |x| pair.1.push(x);
    push(3);
    let __b = &mut pair;
    if __c() {
        __touch(__b);
    } else {
        pair.0.retain(|x| !pair.1.contains(x));
    }
    pair.0.iter_mut().for_each(|x| *x += pair.1.len() as i32);
    println!("{:?} {:?}", pair.0, pair.1);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
