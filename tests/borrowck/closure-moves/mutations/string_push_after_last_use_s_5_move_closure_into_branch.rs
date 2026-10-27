fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    if __c() {
        let __m = move || __see(&s);
        __m();
    }
    s.push('b');
    println!("{n} {s}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
