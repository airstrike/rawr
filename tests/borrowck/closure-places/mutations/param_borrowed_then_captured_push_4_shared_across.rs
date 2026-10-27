fn bump(r: &mut Vec<i32>) {
    let first = &r[0];
    let mut push = |x| r.push(x);
    let __s = &push;
    push(1);
    __see(__s);
    println!("{first}");
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
