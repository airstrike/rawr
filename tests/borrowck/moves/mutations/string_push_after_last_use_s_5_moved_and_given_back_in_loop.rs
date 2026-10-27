fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    for __i in 0..2 {
        let __m = s;
        s = __m;
    }
    s.push('b');
    println!("{n} {s}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
