fn main() {
    let mut v = vec![1, 2];
    let n = v.iter().count();
    v.push(3);
    println!("{n} {v:?}");
}
