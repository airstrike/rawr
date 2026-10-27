fn main() {
    let pair = (None, 1);
    let first = pair;
    let second = pair;
    let typed: (Option<u8>, i32) = first;
    match second {
        __v => __take(__v),
    }
    println!("{:?} {:?}", typed, second);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
