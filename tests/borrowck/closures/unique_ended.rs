fn main() {
    let mut x = 1;
    let r = &mut x;
    let mut c = || *r += 1;
    c();
    let s = &*r;
    println!("{}", s);
}
