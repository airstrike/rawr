fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    __see(&vec![&s].first());
    s.push('b');
    println!("{n} {s}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
