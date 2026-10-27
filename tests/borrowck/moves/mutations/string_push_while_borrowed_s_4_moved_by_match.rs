fn main() {
    let mut s = String::from("a");
    let r = &s;
    match s {
        __v => __take(__v),
    }
    s.push('b');
    println!("{r}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
