fn main() {
    let mut p = (Some(vec![1]), 2, String::from("a"));
    if let (Some(ref mut v), _, ref mut s) = p {
        p.1 = 3;
        v.push(4);
        p.1 += 1;
        s.push('b');
        v.push(5);
    }
    println!("{:?}", p);
}
