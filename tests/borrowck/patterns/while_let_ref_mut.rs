fn main() {
    let mut o = Some(3);
    while let Some(ref mut n) = o {
        if *n == 0 {
            o = None;
        } else {
            *n -= 1;
        }
    }
    println!("{:?}", o);
}
