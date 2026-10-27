fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __m = { s };
    s.push('b');
    println!("{n} {s}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
