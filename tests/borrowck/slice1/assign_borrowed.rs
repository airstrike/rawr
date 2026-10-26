fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    println!("{}", *r + x);
}
