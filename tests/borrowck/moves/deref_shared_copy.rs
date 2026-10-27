fn main() {
    let n = 4;
    let r = &n;
    let m = *r;
    println!("{}", m);
}
