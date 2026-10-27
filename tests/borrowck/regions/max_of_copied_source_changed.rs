fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let m = v.iter().copied().max().unwrap();
    a += 1;
    println!("{m} {a}");
}
