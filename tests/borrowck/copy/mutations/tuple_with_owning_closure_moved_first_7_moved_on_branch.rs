fn main() {
    let name = String::from("ab");
    let greet = move || name.len();
    let pair = (greet, 1);
    let first = pair;
    let second = pair;
    if __c() {
        __take(first);
    }
    println!("{} {}", (first.0)() + first.1, (second.0)());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
