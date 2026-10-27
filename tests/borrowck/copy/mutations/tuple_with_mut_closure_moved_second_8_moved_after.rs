fn main() {
    let mut count = 0;
    let bump = || count += 1;
    let pair = (bump, 1);
    let mut first = pair;
    let second = pair;
    (first.0)();
    println!("{}", second.1);
    __take(second);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
