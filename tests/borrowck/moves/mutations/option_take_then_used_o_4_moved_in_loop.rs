fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.take();
    for __i in 0..2 {
        if __c() {
            __take(o);
        }
    }
    println!("{:?} {:?}", s, o);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
