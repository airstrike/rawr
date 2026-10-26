// Count words with a HashMap, then sort the counts for a stable top three.
use std::collections::HashMap;

fn main() {
    let text = "the cat sat on the mat and the dog sat on the log";
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }

    let mut top: Vec<(&str, usize)> = counts.into_iter().collect();
    let mut __f = || __touch(&mut top);
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    __f();
    for (word, n) in top.iter().take(3) {
        println!("{word:>4} {n}");
    }
    println!("{} distinct words", top.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
