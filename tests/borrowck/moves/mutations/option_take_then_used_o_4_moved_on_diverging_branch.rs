fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.take();
    if __c() {
        __take(o);
        panic!();
    }
    println!("{:?} {:?}", s, o);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
