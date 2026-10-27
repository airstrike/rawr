fn main() {
    let mut x = &0;
    println!("{x}");
    x = &vec![1, 2][0];
    println!("{x}");
}
