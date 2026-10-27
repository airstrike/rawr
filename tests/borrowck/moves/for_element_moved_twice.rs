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
}
