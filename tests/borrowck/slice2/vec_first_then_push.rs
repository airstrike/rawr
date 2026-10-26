fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    v.push(4);
    println!("{f:?}");
}
