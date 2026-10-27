fn main() {
    let mut n = 1;
    let mut c = || n += 1;
    c();
    c();
    let m = n;
    println!("{}", m);
}
