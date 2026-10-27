fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.take();
    let ref __v = o;
    println!("{:?} {:?}", s, o);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
