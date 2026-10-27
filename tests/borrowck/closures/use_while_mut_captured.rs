fn main() {
    let mut n = 1;
    let mut c = || n += 1;
    let m = n;
    c();
    println!("{}", m);
}
