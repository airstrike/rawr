fn take(s: String) -> usize {
    s.len()
}

fn main() {
    let mut total = 0;
    for (word, n) in [(String::from("ab"), 1), (String::from("cde"), 2)] {
        total += take(word) * n;
    }
    match total {
        __v => __take(__v),
    }
    println!("{}", total);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
