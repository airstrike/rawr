fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __b = &mut s;
    s.push('b');
    __touch(__b);
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
