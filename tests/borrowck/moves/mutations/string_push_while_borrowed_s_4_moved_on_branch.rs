fn main() {
    let mut s = String::from("a");
    let r = &s;
    if __c() {
        __take(s);
    }
    s.push('b');
    println!("{r}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
