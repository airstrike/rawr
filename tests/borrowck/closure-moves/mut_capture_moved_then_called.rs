fn main() {
    let mut n = 0;
    let mut c = || n += 1;
    let mut d = c;
    d();
    c();
    println!("{}", n);
}
