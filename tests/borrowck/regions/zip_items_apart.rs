fn main() {
    let a = 1;
    let mut b = 2;
    let v = vec![&a];
    let w = vec![&b];
    let (x, _) = v.iter().copied().zip(w.iter().copied()).next().unwrap();
    b += 1;
    println!("{x} {b}");
}
