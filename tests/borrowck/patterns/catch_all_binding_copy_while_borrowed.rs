fn main() {
    let mut t = (1, 2);
    let m = &mut t;
    let n = match t {
        x => x.0,
    };
    m.0 += 1;
    println!("{} {:?}", n, t);
}
