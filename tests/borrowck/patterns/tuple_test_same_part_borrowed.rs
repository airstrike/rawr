fn main() {
    let mut t = (1, 2);
    let r = &mut t.0;
    let n = match t {
        (1, _) => 10,
        _ => 20,
    };
    *r += 1;
    println!("{} {:?}", n, t);
}
