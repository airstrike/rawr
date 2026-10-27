fn main() {
    let text = String::from("ab");
    let list = vec![1, 2, 3];
    let pair = (&text, &list, "cd");
    let first = pair;
    let second = pair;
    if __c() {
        __take(second);
        panic!();
    }
    println!("{} {:?} {} {}", first.0, second.1, pair.2, pair.1.len());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
