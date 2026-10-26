fn main() {
    let mut v = vec![1, 2];
    let mut c = || {
        let r = &mut v;
        r.push(1);
        v.len()
    };
    println!("{}", c());
    println!("{:?}", v);
}
