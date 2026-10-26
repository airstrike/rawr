fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    println!("{f:?}");
    v.push(4);
    println!("{v:?}");
}
