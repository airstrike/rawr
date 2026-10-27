fn bump(r: &mut Vec<i32>) -> usize {
    let mut push = |x| r.push(x);
    let n = r.len();
    push(1);
    n
}

fn main() {
    let mut v = vec![0];
    let n = bump(&mut v);
    let __s = &v;
    println!("{n} {v:?}");
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
