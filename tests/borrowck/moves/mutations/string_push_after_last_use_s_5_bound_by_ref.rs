fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let ref __v = s;
    s.push('b');
    println!("{n} {s}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
