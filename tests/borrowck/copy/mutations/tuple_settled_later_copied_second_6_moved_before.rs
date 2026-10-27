fn main() {
    let pair = (None, 1);
    let first = pair;
    let second = pair;
    let typed: (Option<u8>, i32) = first;
    __take(second);
    println!("{:?} {:?}", typed, second);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
