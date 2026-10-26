fn main() {
    let mut o = Some(vec![1]);
    if let Some(x) = &mut o {
        x.push(1);
        o = None;
    }
    println!("{:?}", o);
}
