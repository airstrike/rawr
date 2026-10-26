fn main() {
    let mut v = vec![1, 2];
    let w = v.to_vec();
    v.push(3);
    println!("{w:?} {v:?}");
}
