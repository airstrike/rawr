fn main() {
    let mut s = String::from("a");
    let r = &s;
    s.push('b');
    __take(s);
    println!("{r}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
