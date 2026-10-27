fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let w: Vec<&i32> = v.iter().copied().collect();
    a += 1;
    println!("{w:?} {a}");
}
