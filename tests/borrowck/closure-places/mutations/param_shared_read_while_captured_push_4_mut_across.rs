fn bump(r: &mut Vec<i32>) -> usize {
    let mut push = |x| r.push(x);
    let n = r.len();
    let __b = &mut push;
    push(1);
    __touch(__b);
    n
}

fn main() {
    let mut v = vec![0];
    let n = bump(&mut v);
    println!("{n} {v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
