fn main() {
    let mut v = vec![1, 2];
    let it = v.iter();
    v.push(3);
    println!("{}", it.count());
}
