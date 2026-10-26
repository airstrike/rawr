fn main() {
    let mut o = Some(vec![1]);
    let Some(ref mut x) = o else {
        return;
    };
    let c = o.is_some();
    x.push(2);
    let __b = &mut o;
    println!("{:?} {}", o, c);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
