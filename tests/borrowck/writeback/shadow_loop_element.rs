fn main() {
    let mut v = vec![1, 2];
    for x in &mut v {
        let v = 10;
        *x += v;
    }
    println!("{v:?}");
}
