fn main() {
    let mut v = vec![1, 2, 3];
    let s: &mut [i64] = &mut v;
    if let [first, .., last] = s {
        *first += 10;
        *last += 20;
    }
    println!("{:?}", v);
}
