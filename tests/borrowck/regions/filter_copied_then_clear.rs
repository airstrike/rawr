fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let w: Vec<&i32> = v.iter().copied().filter(|x| **x > 3).collect();
    v.clear();
    println!("{w:?}");
}
