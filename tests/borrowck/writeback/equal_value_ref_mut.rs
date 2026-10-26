fn main() {
    let mut o = Some(3);
    if let Some(ref mut x) = o {
        o = Some({
            *x = 9;
            3
        });
    }
    println!("{:?}", o);
}
