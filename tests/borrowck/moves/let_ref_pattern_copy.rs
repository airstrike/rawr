fn main() {
    let r = &5;
    let &n = r;
    println!("{}", n);
}
