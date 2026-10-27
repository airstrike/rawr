fn main() {
    let a = 1;
    let b = 2;
    let mut v = vec![&a, &b];
    let x = v.iter().cloned().next().unwrap();
    v.clear();
    println!("{x}");
}
