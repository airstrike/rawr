fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __b = &mut s;
    for __i in 0..2 {
        __touch(__b);
        s.push('b');
    }
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
