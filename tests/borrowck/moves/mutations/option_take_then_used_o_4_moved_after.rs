fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.take();
    println!("{:?} {:?}", s, o);
    __take(o);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
