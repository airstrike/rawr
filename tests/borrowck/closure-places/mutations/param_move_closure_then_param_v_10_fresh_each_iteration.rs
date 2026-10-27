fn bump(r: &mut Vec<i32>) {
    let mut push = move |x| r.push(x);
    push(1);
    r.push(2);
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    for __i in 0..2 {
        let __b = &mut v;
        __touch(__b);
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
