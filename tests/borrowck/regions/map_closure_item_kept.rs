fn main() {
    let mut a = 1;
    let v = vec![1, 2];
    let w: Vec<&i32> = v.iter().map(|_| &a).collect();
    a += 1;
    println!("{w:?}");
}
