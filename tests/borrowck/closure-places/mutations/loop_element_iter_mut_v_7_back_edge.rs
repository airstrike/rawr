fn main() {
    let mut v = vec![vec![1], vec![2]];
    for (i, list) in v.iter_mut().enumerate() {
        let mut push = |x| list.push(x);
        push(i as i32 * 10);
    }
    let __b = &mut v;
    for __i in 0..2 {
        __touch(__b);
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
