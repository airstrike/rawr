fn main() {
    let mut p: (Option<i64>, i64) = (Some(1), 2);
    match p {
        (Some(ref mut x), _) | (None, ref mut x) => {
            *x += 1;
            p.1 = 5;
        }
    }
    println!("{:?}", p);
}
