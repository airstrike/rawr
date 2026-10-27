fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    s.push('b');
    for __i in 0..2 {
        let __m = s;
        s = __m;
    }
    println!("{n} {s}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
