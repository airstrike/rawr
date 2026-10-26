fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    println!("{}", x + y);
}
