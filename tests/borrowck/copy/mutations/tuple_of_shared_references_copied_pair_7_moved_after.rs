fn main() {
    let text = String::from("ab");
    let list = vec![1, 2, 3];
    let pair = (&text, &list, "cd");
    let first = pair;
    let second = pair;
    println!("{} {:?} {} {}", first.0, second.1, pair.2, pair.1.len());
    __take(pair);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
