fn main() {
    let mut v = vec![1];
    let flag = v.len() > 3;
    let mut c = || v.push(2);
    if flag {
        c();
    }
    println!("{:?}", v);
}
