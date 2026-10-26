fn main() {
    let mut v = vec![1, 2, 3];
    let last = v.last_mut().unwrap();
    println!("{}", v.len());
    *last = 9;
}
