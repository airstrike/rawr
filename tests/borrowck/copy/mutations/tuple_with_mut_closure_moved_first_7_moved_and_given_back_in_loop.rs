fn main() {
    let mut count = 0;
    let bump = || count += 1;
    let pair = (bump, 1);
    let mut first = pair;
    let second = pair;
    for __i in 0..2 {
        let __m = first;
        first = __m;
    }
    (first.0)();
    println!("{}", second.1);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
