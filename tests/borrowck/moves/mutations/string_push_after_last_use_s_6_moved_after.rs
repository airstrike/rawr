fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    s.push('b');
    println!("{n} {s}");
    __take(s);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
