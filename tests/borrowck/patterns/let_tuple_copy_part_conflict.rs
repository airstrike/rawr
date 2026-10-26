fn main() {
    let mut t = (1, 2);
    let r = &mut t.0;
    let (a, _) = t;
    *r += 1;
    println!("{} {:?}", a, t);
}
