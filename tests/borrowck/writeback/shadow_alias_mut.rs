fn main() {
    let mut b = vec![0];
    {
        let r = &mut b;
        let mut b = vec![7];
        r.push(1);
        b.push(8);
        println!("{b:?}");
    }
    println!("{b:?}");
}
