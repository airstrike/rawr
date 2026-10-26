fn main() {
    let mut o = Some(vec![1]);
    if let Some(x) = &mut o {
        o = None;
        x.push(1);
    }
    println!("{:?}", o);
}
