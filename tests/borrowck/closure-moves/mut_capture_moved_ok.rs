fn main() {
    let mut n = 0;
    let c = || n += 1;
    let mut d = c;
    d();
    d();
    println!("{}", n);
}
