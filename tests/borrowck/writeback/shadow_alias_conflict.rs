fn main() {
    let mut b = vec![0];
    let r = &mut b;
    {
        let b = 1;
        r.push(b);
    }
    println!("{:?}", b);
    r.push(2);
}
