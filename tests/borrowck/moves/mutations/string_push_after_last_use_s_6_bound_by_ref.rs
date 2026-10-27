fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    s.push('b');
    let ref __v = s;
    println!("{n} {s}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
