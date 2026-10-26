fn main() {
    let mut o = Some(3);
    while let Some(ref mut n) = o {
        let c = o.is_some();
        if *n == 0 || !c {
            break;
        }
        *n -= 1;
    }
    println!("{:?}", o);
}
