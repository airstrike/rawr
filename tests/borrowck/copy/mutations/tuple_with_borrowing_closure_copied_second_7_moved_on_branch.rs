fn main() {
    let name = String::from("ab");
    let size = || name.len();
    let pair = (size, 1);
    let first = pair;
    let second = pair;
    if __c() {
        __take(second);
    }
    println!("{} {} {}", (first.0)() + first.1, (second.0)(), (pair.0)());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
