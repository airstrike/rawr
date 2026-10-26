fn main() {
    let mut p = (Some(vec![1]), 2);
    if let (Some(ref mut v), _) = p {
        let n = p.0.is_some();
        v.push(4);
        println!("{}", n);
    }
    println!("{:?}", p);
}
