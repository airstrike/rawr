fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __b = &mut s;
    if __c() {
        __touch(__b);
    } else {
        s.push('b');
    }
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
