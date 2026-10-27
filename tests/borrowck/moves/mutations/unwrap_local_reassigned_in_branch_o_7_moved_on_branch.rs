fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.unwrap();
    if s.len() > 1 {
        o = Some(String::from("c"));
    }
    if __c() {
        __take(o);
    }
    println!("{} {:?}", s, o);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
