// This one fails on purpose: rawr refuses what rustc refuses.
fn consume(words: Vec<String>) -> usize {
    words.len()
}

fn main() {
    let words = vec![String::from("rawr"), String::from("runs")];
    let n = consume(words);
    println!("{n} words");
    println!("{words:?}"); // error[E0382]: `words` was moved
}
