fn main() {
    let a = 3;
    let mut v = vec![&a];
    let w: Vec<&&i32> = v.iter().collect();
    v.clear();
    println!("{w:?}");
}
