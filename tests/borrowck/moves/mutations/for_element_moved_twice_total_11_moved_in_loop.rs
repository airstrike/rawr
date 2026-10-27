fn take(s: String) -> usize {
    s.len()
}

fn main() {
    let mut total = 0;
    for word in [String::from("ab"), String::from("cde")] {
        total += take(word);
        total += take(word);
    }
    for __i in 0..2 {
        if __c() {
            __take(total);
        }
    }
    println!("{}", total);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
