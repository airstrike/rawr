fn main() {
    let mut x = 1;
    let v = vec![&x];
    x = 2;
    println!("{}", x);
}
