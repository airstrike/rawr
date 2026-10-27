fn main() {
    let mut x = 1;
    let r = &x;
    match x {
        __v => __take(__v),
    }
    x = 2;
    println!("{}", *r + x);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
