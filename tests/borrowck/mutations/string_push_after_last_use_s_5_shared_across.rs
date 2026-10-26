fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __s = &s;
    s.push('b');
    __see(__s);
    println!("{n} {s}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
