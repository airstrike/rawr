fn main() {
    let mut v = vec![1];
    let mut c = || v.push(1);
    for _ in 0..3 {
        c();
        println!("{}", v.len());
    }
}
