fn main() {
    let mut v = vec![1, 2];
    let n = v.len();
    v.push(3);
    println!("{n} {v:?}");
}
