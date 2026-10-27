fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let mut __f = || __touch(&mut s);
    for __i in 0..2 {
        __f();
        s.push('b');
    }
    println!("{n} {s}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
