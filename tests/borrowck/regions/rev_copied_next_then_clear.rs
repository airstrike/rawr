fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let x = v.iter().copied().rev().next().unwrap();
    v.clear();
    println!("{x}");
}
