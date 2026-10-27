fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    s.push('b');
    let mut __f = || __touch(&mut s);
    let mut __g = __f;
    __g();
    __f();
    println!("{n} {s}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
