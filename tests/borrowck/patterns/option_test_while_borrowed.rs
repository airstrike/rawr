fn main() {
    let mut o = Some(1);
    let m = &mut o;
    let n = match o {
        Some(_) => 1,
        None => 2,
    };
    *m = None;
    println!("{} {:?}", n, o);
}
