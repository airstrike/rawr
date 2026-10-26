// Count words with a HashMap, then sort the counts for a stable top three.
use std::collections::HashMap;

fn main() {
    let text = "the cat sat on the mat and the dog sat on the log";
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }

    let mut top: Vec<(&str, usize)> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (word, n) in top.iter().take(3) {
        println!("{word:>4} {n}");
    }
    let mut __o = Some(&top);
    __see(&__o);
    __o = None;
    println!("{} distinct words", top.len());
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
