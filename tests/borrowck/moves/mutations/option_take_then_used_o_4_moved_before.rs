fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.take();
    __take(o);
    println!("{:?} {:?}", s, o);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
