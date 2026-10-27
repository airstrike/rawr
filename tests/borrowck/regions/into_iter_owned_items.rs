fn main() {
    let a = 3;
    let mut b = 4;
    let v = vec![&a, &b];
    let x = v.into_iter().next().unwrap();
    b += 1;
    println!("{x} {b}");
}
