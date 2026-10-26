fn main() {
    let mut t = (false, 1);
    let r = &mut t.1;
    match t {
        (true, _) => *r += 1,
        (false, 1) => {}
        (false, _) => {}
    }
    println!("{:?}", t);
}
