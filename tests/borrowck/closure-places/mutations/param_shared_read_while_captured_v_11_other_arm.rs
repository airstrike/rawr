fn bump(r: &mut Vec<i32>) -> usize {
    let mut push = |x| r.push(x);
    let n = r.len();
    push(1);
    n
}

fn main() {
    let mut v = vec![0];
    let n = bump(&mut v);
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        println!("{n} {v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
