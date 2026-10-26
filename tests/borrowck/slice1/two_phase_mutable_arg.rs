fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    println!("{:?}", v);
}
