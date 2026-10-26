fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __f = || __see(&s);
    let __g = __f;
    __f();
    s.push('b');
    __g();
    println!("{n} {s}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
