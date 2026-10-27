fn main() {
    let pair = (None, 1);
    let first = pair;
    let second = pair;
    let typed: (Option<String>, i32) = first;
    for __i in 0..2 {
        if __c() {
            __take(second);
        }
    }
    println!("{:?} {:?}", typed, second);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
