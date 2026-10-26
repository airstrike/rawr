fn main() {
    let mut a = 1;
    let refs = vec![&a];
    let f = refs.first();
    a += 1;
    println!("{f:?}");
}
