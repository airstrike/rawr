fn main() {
    let mut s = String::from("a");
    let r = &s;
    __see(&vec![&s].first());
    s.push('b');
    println!("{r}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
