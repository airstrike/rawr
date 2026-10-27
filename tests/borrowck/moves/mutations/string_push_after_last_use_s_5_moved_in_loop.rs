fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    for __i in 0..2 {
        if __c() {
            __take(s);
        }
    }
    s.push('b');
    println!("{n} {s}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
