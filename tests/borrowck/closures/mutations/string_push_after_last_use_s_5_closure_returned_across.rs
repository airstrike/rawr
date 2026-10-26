fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __h = || &s;
    let __k = __h();
    s.push('b');
    __see(__k);
    println!("{n} {s}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
