fn bump(r: &mut Vec<i32>) {
    let mut push = move |x| r.push(x);
    push(1);
    let __b = &mut push;
    __touch(__b);
    r.push(2);
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
