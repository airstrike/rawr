fn main() {
    let mut v = vec![1, 2];
    let c = || v.len() + 1;
    let n = c();
    v.push(3);
    println!("{} {:?}", n, v);
}
