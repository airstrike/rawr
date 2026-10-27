fn main() {
    let mut count = 0;
    let bump = || count += 1;
    let pair = (bump, 1);
    let mut first = pair;
    let second = pair;
    if __c() {
        __take(first);
        panic!();
    }
    (first.0)();
    println!("{}", second.1);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
