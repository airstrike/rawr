fn main() {
    let mut v = vec![1, 2, 3];
    let last = v.last_mut().unwrap();
    *last = 9;
    println!("{} {:?}", v.len(), v);
}
