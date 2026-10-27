fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let mut best = &0;
    for x in &v {
        best = *x;
    }
    a += 1;
    println!("{best} {a}");
}
