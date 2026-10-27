fn main() {
    let a = 3;
    let mut v = vec![&a];
    let (i, x) = v.iter().copied().enumerate().next().unwrap();
    v.clear();
    println!("{i} {x}");
}
