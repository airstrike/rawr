fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.take();
    match o {
        __v => __take(__v),
    }
    println!("{:?} {:?}", s, o);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
