fn main() {
    let mut o = Some(vec![1]);
    if let Some(x) = &mut o {
        o = None;
        x.push(1);
    }
    let __s = &o;
    println!("{:?}", o);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
