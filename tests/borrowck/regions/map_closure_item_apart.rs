fn main() {
    let a = 1;
    let mut v = vec![1, 2];
    let w: Vec<&i32> = v.iter().map(|_| &a).collect();
    v.push(3);
    println!("{w:?}");
}
