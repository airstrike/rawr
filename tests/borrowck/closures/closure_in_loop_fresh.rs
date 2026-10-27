fn main() {
    let mut v = vec![1];
    for i in 0..3 {
        let mut c = || v.push(i);
        c();
        println!("{}", v.len());
    }
}
