fn main() {
    let name = String::from("ab");
    let size = || name.len();
    let pair = (size, 1);
    let first = pair;
    let second = pair;
    for __i in 0..2 {
        if __c() {
            __take(pair);
        }
    }
    println!("{} {} {}", (first.0)() + first.1, (second.0)(), (pair.0)());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
