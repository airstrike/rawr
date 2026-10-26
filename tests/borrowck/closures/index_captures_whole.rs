fn main() {
    let mut v = vec![1, 2];
    let mut c = || v[0] += 1;
    println!("{}", v[1]);
    c();
}
