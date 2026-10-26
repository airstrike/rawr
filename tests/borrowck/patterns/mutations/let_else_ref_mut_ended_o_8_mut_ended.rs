fn main() {
    let mut o = Some(vec![1]);
    let Some(ref mut x) = o else {
        return;
    };
    x.push(2);
    let c = o.is_some();
    let __b = &mut o;
    __touch(__b);
    println!("{:?} {}", o, c);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
