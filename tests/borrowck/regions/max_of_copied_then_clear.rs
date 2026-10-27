fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let m = v.iter().copied().max().unwrap();
    v.clear();
    println!("{m}");
}
