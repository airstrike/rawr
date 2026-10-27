fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    match x {
        __v => __take(__v),
    }
    println!("{}", *r + x);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
