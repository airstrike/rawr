fn main() {
    let mut a = 1;
    let mut v = vec![&a];
    let x = v.pop().unwrap();
    a += 1;
    println!("{x}");
}
