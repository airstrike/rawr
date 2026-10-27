fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    if __c() {
        __take(s);
    }
    s.push('b');
    println!("{n} {s}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
