fn main() {
    let mut x = 1;
    let r = &mut x;
    let s = &*r;
    let mut c = || *r += 1;
    c();
    println!("{}", s);
}
