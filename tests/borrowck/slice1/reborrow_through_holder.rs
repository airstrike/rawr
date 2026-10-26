fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &mut *r;
    s.push(2);
    r.push(3);
    println!("{:?}", v);
}
