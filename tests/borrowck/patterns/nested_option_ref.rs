fn main() {
    let mut p = (Some(vec![1]), 2);
    if let (Some(ref mut v), _) = p {
        p.1 = 3;
        v.push(4);
    }
    println!("{:?}", p);
}
