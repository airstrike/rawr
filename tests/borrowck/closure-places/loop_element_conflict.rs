fn main() {
    let mut v = vec![1, 2, 3];
    for x in &mut v {
        let mut double = || *x *= 2;
        *x += 1;
        double();
    }
    println!("{v:?}");
}
