fn take(s: String) -> usize {
    s.len()
}

fn main() {
    let mut total = 0;
    for word in [String::from("ab"), String::from("cde")] {
        total += take(word);
        total += take(word);
    }
    println!("{}", total);
    __take(total);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
