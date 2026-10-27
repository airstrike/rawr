fn main() {
    let mut v = vec![1, 2];
    let w = vec![3];
    let s = &w;
    let c = || s;
    let r = c();
    v.push(3);
    println!("{:?} {:?}", r, v);
}
