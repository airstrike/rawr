fn main() {
    let mut o = Some(vec![1]);
    if let Some(x) = &mut o {
        o = None;
        x.push(1);
    }
    let __b = &mut o;
    __touch(__b);
    println!("{:?}", o);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
