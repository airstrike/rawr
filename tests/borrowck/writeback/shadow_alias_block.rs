fn main() {
    let mut b = vec![0];
    {
        let r = &mut b;
        let b = 5;
        r.push(1);
        println!("{b}");
    }
    println!("{b:?}");
}
