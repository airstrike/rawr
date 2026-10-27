fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    calls.push(|| v.len());
    v.push(3);
    println!("{}", calls[0]());
}
