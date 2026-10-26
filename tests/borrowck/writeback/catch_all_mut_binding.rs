fn main() {
    let mut v = vec![1];
    match &mut v {
        x => x.push(2),
    }
    println!("{v:?}");
}
