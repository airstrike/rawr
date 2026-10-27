fn main() {
    let mut n = 0;
    let r = &mut n;
    let mut c = || *r += 1;
    let mut d = c;
    d();
    c();
    println!("{}", n);
}
