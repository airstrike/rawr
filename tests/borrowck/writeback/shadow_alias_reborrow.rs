fn main() {
    let mut a = vec![0];
    let b = &mut a;
    {
        let r = &mut *b;
        let b = vec![9];
        r.push(1);
        println!("{b:?}");
    }
    println!("{a:?}");
}
