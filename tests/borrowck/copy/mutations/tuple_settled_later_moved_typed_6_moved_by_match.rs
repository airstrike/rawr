fn main() {
    let pair = (None, 1);
    let first = pair;
    let second = pair;
    let typed: (Option<String>, i32) = first;
    match typed {
        __v => __take(__v),
    }
    println!("{:?} {:?}", typed, second);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
