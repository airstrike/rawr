fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __b = &mut s;
    s.push('b');
    if __c() {
        __touch(__b);
    }
    println!("{n} {s}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
