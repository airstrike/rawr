fn main() {
    let name = String::from("ab");
    let greet = move || name.len();
    let pair = (greet, 1);
    let first = pair;
    let second = pair;
    for __i in 0..2 {
        if __c() {
            __take(second);
        }
    }
    println!("{} {}", (first.0)() + first.1, (second.0)());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
