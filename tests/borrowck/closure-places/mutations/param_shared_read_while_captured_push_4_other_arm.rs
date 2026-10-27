fn bump(r: &mut Vec<i32>) -> usize {
    let mut push = |x| r.push(x);
    let n = r.len();
    let __b = &mut push;
    if __c() {
        __touch(__b);
    } else {
        push(1);
    }
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
