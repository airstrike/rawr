fn main() {
    let mut v = vec![1, 2];
    let w = vec![3];
    let c = |flag: bool| {
        if flag {
            return &v;
        }
        &w
    };
    let r = c(true);
    v.push(3);
    println!("{:?}", r);
}
