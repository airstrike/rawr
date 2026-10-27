fn main() {
    let mut v = vec![1, 2];
    let r = {
        let c = || &v;
        c()
    };
    v.push(3);
    println!("{:?}", r);
}
